use std::sync::Arc;

use negentropy::{Id, Negentropy, NegentropyStorageVector};
use nmp_core::planner::{InterestId, InterestLifecycle};
use nmp_core::store::{RawEvent, VerifiedEvent};
use nmp_core::substrate::{RelayTextInterceptor, ReqFrameContext, ReqFrameInterceptor};
use nmp_core::{Kernel, OutboundMessage, RelayRole};
use nmp_coverage_gate::CoverageGate;
use nostr::{ClientMessage, JsonUtil as _};

use crate::codec::{hex_decode, hex_encode};
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
fn opens_negentropy_for_author_kind_product_at_threshold() {
    let runtime = Arc::new(NegentropySyncRuntime::new(CoverageGate::default()));
    let mut kernel = Kernel::testing_new(50);
    let out = runtime
        .intercept_req(&mut kernel, &ctx(25, &[3, 10_000]))
        .unwrap();
    assert_eq!(out.len(), 1);
    assert!(out[0].text().starts_with(r#"["NEG-OPEN","sub-large","#));
}

#[test]
fn counts_three_kinds_times_twenty_authors() {
    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let mut kernel = Kernel::testing_new(50);
    assert!(runtime
        .intercept_req(&mut kernel, &ctx(20, &[0, 3, 10_002]))
        .is_some());
}

#[test]
fn below_threshold_or_tailing_falls_back_to_raw_req() {
    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let mut kernel = Kernel::testing_new(50);
    assert!(runtime
        .intercept_req(&mut kernel, &ctx(24, &[3, 10_000]))
        .is_none());
    let mut tailing = ctx(50, &[1]);
    tailing.lifecycle = InterestLifecycle::Tailing;
    assert!(runtime.intercept_req(&mut kernel, &tailing).is_none());
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

fn insert_cached_event(
    kernel: &mut Kernel,
    id: [u8; 32],
    author: &str,
    kind: u32,
    created_at: u64,
) {
    let raw = RawEvent {
        id: hex_encode(&id),
        pubkey: author.to_string(),
        created_at,
        kind,
        tags: Vec::new(),
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
