//! K3 Stage A — un-floor NEG-OPEN so reconciliation repairs below-floor gaps.
//!
//! Split out of `runtime_tests.rs` to keep that file under the 500-LOC hard cap
//! (AGENTS.md file-size rule). Self-contained: carries its own small helper set
//! rather than reaching into `runtime_tests` private items.

use negentropy::{Id, Negentropy, NegentropyStorageVector};
use nmp_planner::{InterestId, InterestLifecycle};
use nmp_store::{RawEvent, VerifiedEvent};
use nmp_core::substrate::{RelayTextInterceptor, ReqFrameContext, ReqFrameInterceptor};
use nmp_core::{Kernel, OutboundMessage, RelayRole};
use nmp_coverage_gate::CoverageGate;
use nostr::{ClientMessage, JsonUtil as _};
use serde_json::Value;

use crate::codec::{hex_decode, hex_encode};
use crate::{NegentropySyncRuntime, SyncedItem, FRAME_SIZE_LIMIT};

fn author(n: u8) -> String {
    format!("{n:02x}").repeat(32)
}

fn id(n: u8) -> [u8; 32] {
    [n; 32]
}

fn id_hex(n: u8) -> String {
    format!("{n:02x}").repeat(32)
}

/// A large floored follow-feed REQ context (≥ 50 author×kind fanout), carrying
/// `since` exactly as `apply_watermark_rewrite` would set it for a follow feed.
fn floored_followfeed_ctx(since: u64) -> ReqFrameContext {
    ReqFrameContext {
        role: RelayRole::Content,
        relay_url: "wss://relay.example".to_string(),
        sub_id: "sub-large".to_string(),
        filter_json: serde_json::json!({
            "authors": (0..50).map(|i| author(i as u8)).collect::<Vec<_>>(),
            "kinds": [1],
            "since": since,
        })
        .to_string(),
        interest_id: InterestId(1),
        lifecycle: InterestLifecycle::OneShot,
    }
}

/// K3 Stage A oracle — NEG-OPEN must un-floor `since` so set reconciliation
/// covers the FULL window and can repair a below-floor gap.
///
/// Scenario (the H2 backfill-suppression case from the 16-journey review):
/// the subscription's REQ filter has been watermark-floored to `since = floor`
/// (the newest stored event matching the shape + 1). A relay holds an event
/// authored BELOW that floor that the client never fetched (a gap). With the
/// floor inherited by NEG-OPEN, the reconciliation domain is `[floor, ∞)` and
/// the below-floor relay event can never appear in `need` — the gap is
/// permanently unrepairable. After the fix, NEG-OPEN drops `since`, the
/// reconciliation domain is `[0, ∞)`, and the below-floor event surfaces in the
/// ids-only REQ.
///
/// This test FAILS on pre-fix master (the below-floor id is absent from the ids
/// REQ because the floored `since` excludes it from both `local_items` and the
/// NEG-OPEN filter) and passes after Stage A lands.
#[test]
fn neg_open_unfloors_since_and_repairs_below_floor_gap() {
    let floor: u64 = 5_000;
    // Event the client already has, AT the floor — defines the watermark.
    let at_floor_id = id(0xa1);
    // Event only the relay has, BELOW the floor — the gap to repair.
    let below_floor_id = id(0xb2);

    let mut kernel = Kernel::testing_new(50);
    // Client stores the at-floor event (author 0). This is what the watermark
    // rewrite floored `since` to (floor = at_floor_ts + 1, modelled here as the
    // floor itself for clarity).
    insert_cached_event(&mut kernel, at_floor_id, &author(0), 1, floor);

    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let ctx = floored_followfeed_ctx(floor);

    let opened = runtime
        .intercept_req(&mut kernel, &ctx)
        .expect("large floored filter must open NIP-77");
    assert_eq!(opened.len(), 1, "OneShot opens a single NEG-OPEN");
    assert!(opened[0].text().starts_with(r#"["NEG-OPEN","sub-large","#));

    // Relay's full candidate set: the at-floor event (shared) AND a below-floor
    // event (the gap only the relay has).
    let relay_candidates = vec![
        SyncedItem {
            created_at: floor,
            id: at_floor_id,
        },
        SyncedItem {
            created_at: floor - 1_000, // BELOW the floor
            id: below_floor_id,
        },
    ];
    // A spec-compliant relay scopes its reconciliation set to the NEG-OPEN
    // filter window: it only offers events whose `created_at >= since`. THIS is
    // the mechanism the floor poisons — if NEG-OPEN inherits `since = floor`,
    // the relay never offers the below-floor event, so the gap is unrepairable.
    // The fix strips `since` from NEG-OPEN, so the relay window is `[0, ∞)`.
    let neg_open_since = neg_open_filter_since(opened[0].text());
    let relay_items: Vec<SyncedItem> = relay_candidates
        .into_iter()
        .filter(|item| neg_open_since.map_or(true, |s| item.created_at >= s))
        .collect();
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

    let ids_req = final_out
        .iter()
        .map(OutboundMessage::text)
        .find(|text| text.starts_with(r#"["REQ","sub-large","#))
        .expect("below-floor gap must be fetched by an ids-only REQ");
    assert!(
        ids_req.contains(&id_hex(0xb2)),
        "the below-floor relay event MUST be reconciled and requested — NEG-OPEN \
         must not inherit the watermark floor (got: {ids_req})"
    );
}

/// Guard the contained-blast-radius half of Stage A: the *fallback* plain REQ
/// (sent when the relay rejects NEG) MUST keep the floored `since`. Un-flooring
/// is a NEG-only widening; a plain REQ still wants the floor to avoid
/// re-fetching cached events.
#[test]
fn fallback_req_after_neg_err_keeps_the_floor() {
    let floor: u64 = 7_000;
    let mut kernel = Kernel::testing_new(50);
    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let ctx = floored_followfeed_ctx(floor);

    assert!(runtime.intercept_req(&mut kernel, &ctx).is_some());
    let out = runtime.on_relay_text(
        &mut kernel,
        "wss://relay.example",
        r#"["NEG-ERR","sub-large","unsupported"]"#,
    );
    assert_eq!(out.len(), 1, "NEG-ERR yields one fallback REQ");
    let value: Value = serde_json::from_str(out[0].text()).expect("fallback REQ JSON");
    assert_eq!(value[0], Value::from("REQ"));
    assert_eq!(
        value[2].get("since").and_then(Value::as_u64),
        Some(floor),
        "the fallback plain REQ must retain the watermark floor"
    );
}

/// K3 Stage B2 ORACLE — a relay that accepts NEG-OPEN then goes SILENT (no
/// NEG-MSG, no NEG-ERR, no NOTICE) must not starve the interest: after the
/// liveness deadline elapses, the idle sweep falls back to a plain REQ.
///
/// Scenario: `intercept_req` opens a NEG session (the wire sub is "live" with
/// no EOSE). The relay never responds. Without a liveness deadline the session
/// sits in `Probing` forever and the interest is never served. The fix stamps
/// `opened_at_secs` at open and `on_idle_tick` (the actor's wall-clock-gated
/// idle hook — D8, no sleep/loop) falls back to the plain REQ once
/// `now − opened_at ≥ NEG_OPEN_LIVENESS_DEADLINE_SECS`.
///
/// A `MonotonicSecondClock` is installed so the deadline can be crossed by
/// advancing the kernel clock — no real sleep. The companion assertions confirm
/// the sweep does NOT fire before the deadline (a live-but-slow relay is not
/// torn down).
///
/// FAILS on pre-B2 master (`on_idle_tick` is the no-op default → no REQ ever),
/// passes after the liveness deadline lands.
#[test]
fn silent_relay_after_neg_open_falls_back_to_req_at_deadline() {
    use nmp_core::time::{Duration, UNIX_EPOCH};
    use nmp_core::MonotonicSecondClock;
    use std::sync::Arc;

    let floor: u64 = 9_000;
    let mut kernel = Kernel::testing_new(50);

    // Install an advanceable clock anchored well past the epoch.
    let clock = Arc::new(MonotonicSecondClock::new(
        UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    ));
    kernel.set_clock(clock.clone());

    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let ctx = floored_followfeed_ctx(floor);

    // Open the NEG session. The relay then goes completely silent.
    let opened = runtime
        .intercept_req(&mut kernel, &ctx)
        .expect("large floored filter must open NIP-77");
    assert_eq!(opened.len(), 1, "OneShot opens a single NEG-OPEN");

    // Before the deadline: the idle sweep must NOT tear the session down.
    clock.advance_secs(29);
    let early = runtime.on_idle_tick(&mut kernel);
    assert!(
        early.is_empty(),
        "a NEG session under the liveness deadline must not fall back yet \
         (a slow-but-live relay must be left alone); got {early:?}"
    );

    // Cross the 30 s deadline: the sweep falls back to a plain REQ.
    clock.advance_secs(1);
    let out = runtime.on_idle_tick(&mut kernel);
    assert_eq!(
        out.len(),
        1,
        "after the liveness deadline a silent NEG relay must emit exactly one \
         fallback REQ for the starved interest; got {out:?}"
    );
    let value: Value = serde_json::from_str(out[0].text()).expect("fallback REQ JSON");
    assert_eq!(value[0], Value::from("REQ"), "fallback must be a plain REQ");
    assert_eq!(
        value[1], "sub-large",
        "fallback REQ must reuse the starved interest's sub id"
    );
    // The fallback plain REQ retains the watermark floor (same contract as the
    // NEG-ERR fallback — un-flooring is a NEG-only widening, Stage A).
    assert_eq!(
        value[2].get("since").and_then(Value::as_u64),
        Some(floor),
        "the liveness-deadline fallback REQ must retain the watermark floor"
    );

    // Idempotent: the session was removed, so a second sweep emits nothing
    // (no duplicate REQ storm on every subsequent idle tick).
    clock.advance_secs(60);
    assert!(
        runtime.on_idle_tick(&mut kernel).is_empty(),
        "the timed-out session must be removed so the sweep does not re-emit"
    );
}

/// B2 companion — a relay that RESPONDED (drove the reconciliation to
/// completion) must never trigger a spurious liveness fallback. The NEG-MSG
/// terminal removes the session, so a much-later idle sweep emits nothing: the
/// deadline only ever fires for genuinely silent relays, never as a duplicate
/// of a completed sync.
#[test]
fn responded_relay_never_triggers_liveness_fallback() {
    use nmp_core::time::{Duration, UNIX_EPOCH};
    use nmp_core::MonotonicSecondClock;
    use std::sync::Arc;

    let mut kernel = Kernel::testing_new(50);
    let clock = Arc::new(MonotonicSecondClock::new(
        UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    ));
    kernel.set_clock(clock.clone());

    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let ctx = floored_followfeed_ctx(9_000);
    let opened = runtime
        .intercept_req(&mut kernel, &ctx)
        .expect("NEG-OPEN opens");

    // The relay responds with a NEG-MSG that completes the reconciliation
    // (empty server set → the single round resolves to Done, session removed).
    let mut server = negentropy_server(Vec::new());
    let client_payload = client_neg_payload(opened[0].text());
    let server_payload = server.reconcile(&client_payload).expect("server reconcile");
    let relay_msg = format!(
        r#"["NEG-MSG","sub-large","{}"]"#,
        hex_encode(&server_payload)
    );
    let _ = runtime.on_relay_text(&mut kernel, "wss://relay.example", &relay_msg);

    // Long after the deadline window: no session remains, so the sweep is a
    // no-op — a completed sync must never be re-issued as a fallback REQ.
    clock.advance_secs(120);
    assert!(
        runtime.on_idle_tick(&mut kernel).is_empty(),
        "a relay that responded and completed reconciliation must never trigger \
         a liveness fallback — the deadline is only for silent relays"
    );
}

// ─── K3 Stage D2: coverage-gate staleness (ADR-0056 §3.D2) ──────────────────────
//
// The gate now consults coverage-ledger staleness, not fanout alone: a
// `(filter_hash, relay)` with no completed-coverage row is "uncovered" and
// should prefer the full-window negentropy reconciliation regardless of fanout
// (it is the self-healer for below-floor gaps). A covered shape, or the flag
// off, leaves the fanout heuristic as the sole gate.

/// A SMALL-fanout REQ (1 author × 1 kind = 1 pair, far below the 50-pair fanout
/// threshold). On fanout alone this NEVER opens negentropy. Its `sub-<hash>` id
/// is what the staleness gate keys on.
fn small_followfeed_ctx() -> ReqFrameContext {
    ReqFrameContext {
        role: RelayRole::Content,
        relay_url: "wss://relay.example".to_string(),
        sub_id: "sub-smallhash".to_string(),
        filter_json: serde_json::json!({
            "authors": [author(0)],
            "kinds": [1],
        })
        .to_string(),
        interest_id: InterestId(1),
        lifecycle: InterestLifecycle::OneShot,
    }
}

/// NO coverage row ⇒ a SMALL filter (below the fanout threshold) is "uncovered"
/// and the staleness gate opens negentropy anyway — the self-heal path for a
/// never-synced shape.
#[test]
fn uncovered_small_filter_opens_negentropy_when_ledger_enabled() {
    let mut kernel = Kernel::testing_new(50);
    // No coverage row recorded for ("smallhash", relay) → uncovered.

    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let opened = runtime.intercept_req(&mut kernel, &small_followfeed_ctx());

    let opened = opened.expect(
        "an uncovered (no-ledger-row) shape must open NIP-77 even below the fanout \
         threshold — the full-window reconciliation is the below-floor self-healer",
    );
    assert!(
        opened
            .iter()
            .any(|m| m.text().starts_with(r#"["NEG-OPEN","sub-smallhash","#)),
        "expected a NEG-OPEN for the uncovered small filter",
    );
}

/// Flag ON + a coverage row PRESENT ⇒ the shape is covered, so the staleness
/// path does NOT force negentropy; the small filter falls through to the fanout
/// gate and (being below threshold) opens NO negentropy. Proves the gate is
/// scoped to genuinely-uncovered shapes, not a blanket "always negentropy".
#[test]
fn covered_small_filter_does_not_force_negentropy() {
    let mut kernel = Kernel::testing_new(50);
    // Record completed coverage for this (filter_hash, relay) → covered.
    kernel
        .event_store_handle()
        .record_coverage("smallhash", "wss://relay.example", 1_700_000_000);

    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let opened = runtime.intercept_req(&mut kernel, &small_followfeed_ctx());

    assert!(
        opened.is_none(),
        "a COVERED small filter must fall through to the fanout gate (below \
         threshold ⇒ plain REQ, no negentropy) — staleness must not blanket-force it",
    );
}

// ─── helpers (self-contained for this module) ────────────────────────────────

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

/// Extract the `since` bound (if any) from the filter of a `NEG-OPEN` frame.
/// Wire shape: `["NEG-OPEN", <sub>, <filter>, <hex>]`. The relay reconciliation
/// mock uses this to scope which stored events it offers — modelling a
/// spec-compliant relay that honours the NEG-OPEN filter window.
fn neg_open_filter_since(text: &str) -> Option<u64> {
    let value: Value = serde_json::from_str(text).expect("NEG-OPEN JSON");
    assert_eq!(value[0], Value::from("NEG-OPEN"));
    value[2].get("since").and_then(Value::as_u64)
}
