use std::sync::Arc;

use negentropy::{Id, Negentropy, NegentropyStorageVector};
use nmp_core::substrate::{RelayTextInterceptor, ReqFrameContext, ReqFrameInterceptor};
use nmp_core::{Kernel, OutboundMessage};
use nmp_coverage_gate::CoverageGate;
use nmp_network::role::RelayRole;
use nmp_planner::{InterestId, InterestLifecycle};
use nmp_store::{RawEvent, VerifiedEvent};
use nostr::{ClientMessage, JsonUtil as _};
use serde_json::Value;

use crate::codec::{hex_decode, hex_decode_size_limited, hex_encode};
use crate::{NegentropySyncRuntime, RelayNegentropyState, SyncedItem, FRAME_SIZE_LIMIT};

fn author(n: u8) -> String {
    format!("{n:02x}").repeat(32)
}

fn id(n: u8) -> [u8; 32] {
    [n; 32]
}

fn id_hex(n: u8) -> String {
    format!("{n:02x}").repeat(32)
}

fn ctx(authors: usize, kinds: &[u32]) -> ReqFrameContext {
    ReqFrameContext {
        role: RelayRole::Content,
        relay_url: "wss://relay.example".to_string(),
        sub_id: "sub-large".to_string(),
        filter_json: serde_json::json!({
            "authors": (0..authors).map(|i| author(i as u8)).collect::<Vec<_>>(),
            "kinds": kinds,
        })
        .to_string(),
        interest_id: InterestId(1),
        lifecycle: InterestLifecycle::OneShot,
    }
}

#[test]
fn opens_negentropy_for_known_replaceable_surface_at_threshold() {
    let runtime = Arc::new(NegentropySyncRuntime::new(CoverageGate::default()));
    let mut kernel = Kernel::testing_new(50);
    let out = runtime
        .intercept_req(&mut kernel, &ctx(25, &[3, 10_000]))
        .unwrap();
    assert_eq!(out.len(), 1);
    assert!(out[0].text().starts_with(r#"["NEG-OPEN","sub-large","#));
}

#[test]
fn known_multi_kind_replaceable_surface_above_threshold() {
    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let mut kernel = Kernel::testing_new(50);
    assert!(runtime
        .intercept_req(&mut kernel, &ctx(20, &[0, 3, 10_002]))
        .is_some());
}

#[test]
fn tailing_large_filter_opens_live_only_req_and_negentropy_backfill() {
    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let mut kernel = Kernel::testing_new(50);
    let mut tailing = ctx(50, &[1]);
    tailing.lifecycle = InterestLifecycle::Tailing;
    tailing.filter_json = serde_json::json!({
        "authors": (0..50).map(|i| author(i as u8)).collect::<Vec<_>>(),
        "kinds": [1],
        "limit": 200,
    })
    .to_string();

    let out = runtime
        .intercept_req(&mut kernel, &tailing)
        .expect("large tailing filter must be split");
    assert_eq!(out.len(), 2);

    let (live_sub, live_filter) = req_parts(out[0].text());
    assert_eq!(live_sub, "sub-large");
    assert_eq!(live_filter["limit"], Value::from(0));
    assert_eq!(live_filter["kinds"], serde_json::json!([1]));

    let neg_open_filter = frame_filter(out[1].text(), "NEG-OPEN");
    assert_eq!(
        neg_open_filter["limit"],
        Value::from(200),
        "NIP-77 backfill must reconcile the original bounded stored set"
    );
}

#[test]
fn tailing_neg_err_falls_back_to_original_req_after_live_only_probe() {
    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let mut kernel = Kernel::testing_new(50);
    let mut tailing = ctx(50, &[1]);
    tailing.lifecycle = InterestLifecycle::Tailing;
    tailing.filter_json = serde_json::json!({
        "authors": (0..50).map(|i| author(i as u8)).collect::<Vec<_>>(),
        "kinds": [1],
        "limit": 200,
    })
    .to_string();
    assert_eq!(
        runtime.intercept_req(&mut kernel, &tailing).unwrap().len(),
        2
    );

    let out = runtime.on_relay_text(
        &mut kernel,
        "wss://relay.example",
        r#"["NEG-ERR","sub-large","unsupported"]"#,
    );
    assert_eq!(out.len(), 1);
    let (sub_id, filter) = req_parts(out[0].text());
    assert_eq!(sub_id, "sub-large");
    assert_eq!(filter["limit"], Value::from(200));
}

#[test]
fn neg_err_falls_back_to_original_req_and_marks_unsupported() {
    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let mut kernel = Kernel::testing_new(50);
    let ctx = ctx(50, &[3]);
    assert!(runtime.intercept_req(&mut kernel, &ctx).is_some());
    let out = runtime.on_relay_text(
        &mut kernel,
        "wss://relay.example",
        r#"["NEG-ERR","sub-large","unsupported"]"#,
    );
    assert_eq!(out.len(), 1);
    assert!(out[0].text().starts_with(r#"["REQ","sub-large","#));
    assert_eq!(
        runtime.relay_state("wss://relay.example"),
        RelayNegentropyState::Unsupported
    );
}

#[test]
fn fresh_runtime_uses_cached_store_items_and_fetches_only_missing_ids() {
    let cached_id = id(0xa1);
    let missing_id = id(0xb2);
    let cached_author = author(0);

    let mut kernel = Kernel::testing_new(50);
    insert_cached_event(&mut kernel, cached_id, &cached_author, 3, 1_000);

    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let mut ctx = ctx(25, &[3, 10_000]);
    ctx.filter_json = serde_json::json!({
        "authors": (0..25).map(|i| author(i as u8)).collect::<Vec<_>>(),
        "kinds": [3, 10_000],
    })
    .to_string();

    let opened = runtime
        .intercept_req(&mut kernel, &ctx)
        .expect("large exact filter must open NIP-77");
    assert_eq!(opened.len(), 1);

    let relay_items = vec![
        SyncedItem {
            created_at: 1_000,
            id: cached_id,
        },
        SyncedItem {
            created_at: 2_000,
            id: missing_id,
        },
    ];
    let mut server = negentropy_server(relay_items);
    let mut client_payload = client_neg_payload(opened[0].text());

    let final_out = loop {
        let server_payload = server.reconcile(&client_payload).expect("server reconcile");
        let relay_msg = format!(
            r#"["NEG-MSG","sub-large","{}"]"#,
            hex_encode(&server_payload)
        );
        let out = runtime.on_relay_text(&mut kernel, "wss://relay.example", &relay_msg);
        if let Some(next) = out.iter().find(|msg| is_client_neg_msg(msg.text())) {
            client_payload = client_neg_payload(next.text());
        } else {
            break out;
        }
    };

    assert!(
        final_out
            .iter()
            .any(|msg| msg.text().starts_with(r#"["NEG-CLOSE","sub-large"]"#)),
        "successful reconciliation must close the NIP-77 session"
    );
    let ids_req = final_out
        .iter()
        .map(OutboundMessage::text)
        .find(|text| text.starts_with(r#"["REQ","sub-large","#))
        .expect("missing relay-side events must be fetched by ids-only REQ");
    assert!(
        ids_req.contains(&id_hex(0xb2)),
        "missing relay-side event id must be requested"
    );
    assert!(
        !ids_req.contains(&id_hex(0xa1)),
        "cached event id must not be requested again after reboot"
    );
}

#[test]
fn tailing_backfill_fetches_missing_ids_without_replacing_live_sub() {
    let missing_id = id(0xb2);
    let mut kernel = Kernel::testing_new(50);
    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let mut tailing = ctx(50, &[1]);
    tailing.lifecycle = InterestLifecycle::Tailing;
    tailing.filter_json = serde_json::json!({
        "authors": (0..50).map(|i| author(i as u8)).collect::<Vec<_>>(),
        "kinds": [1],
        "limit": 200,
    })
    .to_string();

    let opened = runtime
        .intercept_req(&mut kernel, &tailing)
        .expect("large tailing filter must be split");
    assert_eq!(opened.len(), 2);
    assert!(opened[0].text().starts_with(r#"["REQ","sub-large","#));
    assert!(opened[1].text().starts_with(r#"["NEG-OPEN","sub-large","#));

    let relay_items = vec![SyncedItem {
        created_at: 2_000,
        id: missing_id,
    }];
    let mut server = negentropy_server(relay_items);
    let mut client_payload = client_neg_payload(opened[1].text());

    let final_out = loop {
        let server_payload = server.reconcile(&client_payload).expect("server reconcile");
        let relay_msg = format!(
            r#"["NEG-MSG","sub-large","{}"]"#,
            hex_encode(&server_payload)
        );
        let out = runtime.on_relay_text(&mut kernel, "wss://relay.example", &relay_msg);
        if let Some(next) = out.iter().find(|msg| is_client_neg_msg(msg.text())) {
            client_payload = client_neg_payload(next.text());
        } else {
            break out;
        }
    };

    assert!(
        final_out
            .iter()
            .any(|msg| msg.text().starts_with(r#"["NEG-CLOSE","sub-large"]"#)),
        "tailing backfill must still close the NIP-77 session"
    );
    let ids_req = final_out
        .iter()
        .map(OutboundMessage::text)
        .find(|text| text.starts_with(r#"["REQ","#))
        .expect("missing relay-side events must be fetched by ids-only REQ");
    let (ids_sub, ids_filter) = req_parts(ids_req);
    assert_eq!(
        ids_sub, "sub-large-neg-ids",
        "ids fetch must not replace the live tailing sub"
    );
    assert!(ids_filter["ids"].as_array().unwrap()[0]
        .as_str()
        .unwrap()
        .contains(&id_hex(0xb2)));
}

/// GAP-5: after a successful reconcile, the kernel's `NegentropySyncStats`
/// must have non-zero `rounds`, `have_ids`, `need_ids`, and
/// `transfer_avoided_bytes` (kernel-computed as `(local − have) × 512`).
///
/// Setup: client has event_A and event_B; relay has event_B and event_C.
/// After reconcile:
///   `have` = [event_A] (client→relay, 1 ID)   → have_ids = 1
///   `need` = [event_C] (relay→client, 1 ID)   → need_ids = 1
///   `local_item_count` = 2
///   `transfer_avoided_bytes` = (2 − 1) × 512 = 512  (event_B in both, no re-fetch)
#[test]
fn done_reconcile_pushes_non_zero_session_stats_to_kernel() {
    let event_a = id(0xa1); // client-only
    let event_b = id(0xb2); // both client and relay
    let event_c = id(0xc3); // relay-only

    let mut kernel = Kernel::testing_new(50);
    // Insert both event_A and event_B into the kernel's local store so the
    // reconciler loads 2 items (`local_item_count = 2`).
    insert_cached_event(&mut kernel, event_a, &author(0), 3, 1_000);
    insert_cached_event(&mut kernel, event_b, &author(1), 3, 2_000);

    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let ctx = ctx(25, &[3, 10_000]);

    let opened = runtime
        .intercept_req(&mut kernel, &ctx)
        .expect("large filter must open NIP-77");
    assert_eq!(opened.len(), 1);

    // Relay server has event_B (shared) and event_C (relay-only).
    let relay_items = vec![
        SyncedItem {
            created_at: 2_000,
            id: event_b,
        },
        SyncedItem {
            created_at: 3_000,
            id: event_c,
        },
    ];
    let mut server = negentropy_server(relay_items);
    let mut client_payload = client_neg_payload(opened[0].text());

    loop {
        let server_payload = server.reconcile(&client_payload).expect("server reconcile");
        let relay_msg = format!(
            r#"["NEG-MSG","sub-large","{}"]"#,
            hex_encode(&server_payload)
        );
        let out = runtime.on_relay_text(&mut kernel, "wss://relay.example", &relay_msg);
        if let Some(next) = out.iter().find(|msg| is_client_neg_msg(msg.text())) {
            client_payload = client_neg_payload(next.text());
        } else {
            break;
        }
    }

    // After Done, the kernel must record non-zero session stats.
    let rounds = kernel.negentropy_sync_rounds_for_test();
    let have = kernel.negentropy_sync_have_ids_for_test();
    let need = kernel.negentropy_sync_need_ids_for_test();
    let avoided = kernel.negentropy_sync_transfer_avoided_bytes_for_test();

    assert!(
        rounds > 0,
        "rounds must be non-zero after a completed session, got {rounds}"
    );
    assert!(
        have > 0,
        "have_ids must be non-zero (event_A is client-only), got {have}"
    );
    assert!(
        need > 0,
        "need_ids must be non-zero (event_C is relay-only), got {need}"
    );
    assert!(
        avoided > 0,
        "transfer_avoided_bytes must be non-zero (event_B in both — no re-fetch), got {avoided}"
    );
}

fn insert_cached_event(
    kernel: &mut Kernel,
    id: [u8; 32],
    author: &str,
    kind: u32,
    created_at: u64,
) {
    insert_cached_event_with_tags(kernel, id, author, kind, created_at, Vec::new());
}

fn insert_cached_event_with_tags(
    kernel: &mut Kernel,
    id: [u8; 32],
    author: &str,
    kind: u32,
    created_at: u64,
    tags: Vec<Vec<String>>,
) {
    let raw = RawEvent {
        id: hex_encode(&id),
        pubkey: author.to_string(),
        created_at,
        kind,
        tags,
        content: String::new(),
        sig: "a".repeat(128),
    };
    kernel
        .event_store_handle()
        .insert(
            VerifiedEvent::from_raw_unchecked(raw),
            &"wss://cache.example".to_string(),
            created_at.saturating_mul(1_000),
        )
        .expect("cache insert");
}

fn negentropy_server(items: Vec<SyncedItem>) -> Negentropy<'static, NegentropyStorageVector> {
    let mut storage = NegentropyStorageVector::with_capacity(items.len());
    for item in items {
        storage
            .insert(item.created_at, Id::from_byte_array(item.id))
            .expect("server insert");
    }
    storage.seal().expect("server storage seal");
    Negentropy::owned(storage, FRAME_SIZE_LIMIT).expect("server negentropy")
}

fn client_neg_payload(text: &str) -> Vec<u8> {
    match ClientMessage::from_json(text).expect("client NIP-77 message") {
        ClientMessage::NegOpen {
            initial_message, ..
        } => hex_decode(&initial_message).expect("NEG-OPEN payload hex"),
        ClientMessage::NegMsg { message, .. } => hex_decode(&message).expect("NEG-MSG payload hex"),
        other => panic!("expected client negentropy message, got {other:?}"),
    }
}

fn is_client_neg_msg(text: &str) -> bool {
    matches!(
        ClientMessage::from_json(text),
        Ok(ClientMessage::NegMsg { .. })
    )
}

fn req_parts(text: &str) -> (String, Value) {
    let value: Value = serde_json::from_str(text).expect("REQ JSON");
    assert_eq!(value[0], Value::from("REQ"));
    (
        value[1].as_str().expect("REQ sub id").to_string(),
        value[2].clone(),
    )
}

fn frame_filter(text: &str, verb: &str) -> Value {
    let value: Value = serde_json::from_str(text).expect("client message JSON");
    assert_eq!(value[0], Value::from(verb));
    value[2].clone()
}

/// GAP-6: a relay-controlled `NEG-MSG` whose hex payload exceeds
/// `FRAME_SIZE_LIMIT * 2` characters must be rejected with a size-gate error
/// before the hex-decode allocation, and the runtime must fall back to the
/// plain `REQ` path.
///
/// The pre-fix code called `hex_decode` on the raw `message` field
/// immediately, which did `Vec::with_capacity(s.len()/2)` before any size
/// check — a relay could send ~64 MiB of hex (tungstenite frame limit) and
/// trigger a ~32 MiB alloc per message.  The fix rejects oversize payloads
/// via a length check before `hex_decode` is called at all.
///
/// This test verifies the runtime-level behavior (fallback REQ) AND that the
/// codec-level size gate fires on the oversize string.
#[test]
fn oversize_neg_msg_falls_back_without_giant_alloc() {
    // Build an oversized hex string: FRAME_SIZE_LIMIT*2 + 2 hex chars.
    let oversize_len = (FRAME_SIZE_LIMIT as usize) * 2 + 2;
    // A string of 'a' repeated is valid lowercase hex but far too large.
    let oversize_hex = "aa".repeat(oversize_len / 2);
    assert!(oversize_hex.len() > FRAME_SIZE_LIMIT as usize * 2);

    // Verify the codec-level gate rejects before allocating.
    assert!(
        hex_decode_size_limited(&oversize_hex).is_err(),
        "codec size gate must reject oversize hex before alloc"
    );

    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let mut kernel = Kernel::testing_new(50);

    // Open a NIP-77 session so the runtime has state for "sub-large".
    let opened = runtime
        .intercept_req(&mut kernel, &ctx(50, &[3]))
        .expect("large filter must open NIP-77");
    assert_eq!(opened.len(), 1);

    // Deliver the oversize NEG-MSG from the relay.
    let relay_msg = format!(r#"["NEG-MSG","sub-large","{}"]"#, oversize_hex);
    let out = runtime.on_relay_text(&mut kernel, "wss://relay.example", &relay_msg);

    // Must fall back to a plain REQ, not return empty or panic.
    assert_eq!(out.len(), 1, "oversize NEG-MSG must produce a fallback REQ");
    assert!(
        out[0].text().starts_with(r#"["REQ","sub-large","#),
        "fallback must be a REQ, got: {}",
        out[0].text()
    );
}

/// Within-limit NEG-MSG must still decode and reconcile normally (the size
/// gate must not block legitimate messages).
#[test]
fn normal_size_neg_msg_is_not_rejected_by_size_gate() {
    // A small valid hex payload (16 bytes = 32 hex chars) is well within limit.
    let small_hex = "aa".repeat(16);
    assert!(small_hex.len() <= FRAME_SIZE_LIMIT as usize * 2);
    assert!(
        hex_decode_size_limited(&small_hex).is_ok(),
        "size gate must not block within-limit hex"
    );
}
