//! Raw signed-event tap fan-out at the single all-kinds ingest seam.
//!
//! Drives a REAL Schnorr-signed event through the kernel's `handle_event`
//! ingest path (the single all-kinds entry, NOT the kind:1/6-only
//! `ingest_pre_verified_event` test-support path the kernel-event observer
//! tests use) and asserts:
//!
//! 1. a Rust raw observer with a matching kind filter receives the
//!    byte-faithful flat NIP-01 JSON including a valid `sig`;
//! 2. an event whose kind is NOT in the filter is dropped;
//! 3. an unverifiable event (bad sig) never reaches the tap.
//!
//! The fan-out path is shared with production: `handle_event` makes the
//! same post-store raw observer call after the existing Schnorr + id-hash
//! gate and store provenance update for any tapped kind. Generic capability
//! (D0) — no protocol nouns.

use super::*;
use crate::actor::{
    new_raw_event_observer_slot, register_rust_raw_observer, KindFilter, RawEventObserver,
};
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use std::sync::{Arc, Mutex};

struct CapturingRawObserver {
    seen: Mutex<Vec<(u32, String, Option<String>)>>,
}

impl CapturingRawObserver {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            seen: Mutex::new(Vec::new()),
        })
    }
}

impl RawEventObserver for CapturingRawObserver {
    fn on_raw_event(&self, kind: u32, json: &str) {
        self.seen
            .lock()
            .unwrap()
            .push((kind, json.to_string(), None));
    }

    fn on_raw_event_with_source(&self, kind: u32, json: &str, source_relay_url: Option<&str>) {
        self.seen.lock().unwrap().push((
            kind,
            json.to_string(),
            source_relay_url.map(ToOwned::to_owned),
        ));
    }
}

/// Build a real Schnorr-signed event of `kind` and return its NIP-01 JSON
/// `Value` (the exact shape the wire delivers and `handle_event` parses).
/// Uses the same `nostr::Keys::generate() + EventBuilder + sign_with_keys`
/// pattern as `ffi/testing.rs::nmp_app_inject_signed_events`.
fn signed_event_value(kind: u32, content: &str) -> serde_json::Value {
    use ::nostr::{EventBuilder, Keys, Kind};
    let keys = Keys::generate();
    let nostr_event = EventBuilder::new(Kind::from(kind as u16), content)
        .sign_with_keys(&keys)
        .expect("sign");
    let tags: Vec<Vec<String>> = nostr_event
        .tags
        .iter()
        .map(|t| t.as_slice().to_vec())
        .collect();
    serde_json::json!({
        "id": nostr_event.id.to_hex(),
        "pubkey": nostr_event.pubkey.to_hex(),
        "created_at": nostr_event.created_at.as_secs(),
        "kind": nostr_event.kind.as_u16(),
        "tags": tags,
        "content": nostr_event.content.clone(),
        "sig": nostr_event.sig.to_string(),
    })
}

#[test]
fn raw_tap_receives_verbatim_signed_event_through_handle_event() {
    let slot = new_raw_event_observer_slot();
    let observer = CapturingRawObserver::new();
    // Filter on kind:1 only — exercises the kind-filter path AND confirms
    // a matching kind is delivered through the all-kinds ingest seam.
    register_rust_raw_observer(&slot, KindFilter::from_kinds([1u32]), observer.clone());

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_raw_event_observers_handle(slot);

    let value = signed_event_value(1, "verbatim tap content");
    kernel.handle_event(
        RelayRole::Content,
        "wss://relay.test",
        "diag-firehose-raw-tap",
        &value,
    );

    let seen = observer.seen.lock().unwrap();
    assert_eq!(
        seen.len(),
        1,
        "exactly one tap delivery for the matching kind"
    );
    let (kind, json, source) = &seen[0];
    assert_eq!(*kind, 1);
    assert_eq!(
        source.as_deref(),
        Some("wss://relay.test"),
        "raw observers must receive the relay URL persisted as source provenance"
    );

    // Byte-faithful: the delivered JSON round-trips to the SAME id /
    // pubkey / sig the wire event carried, and the sig is a real 128-hex
    // Schnorr signature (not a placeholder).
    let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(parsed["id"], value["id"]);
    assert_eq!(parsed["pubkey"], value["pubkey"]);
    assert_eq!(parsed["sig"], value["sig"]);
    assert_eq!(parsed["content"], "verbatim tap content");
    let sig = parsed["sig"].as_str().unwrap();
    assert_eq!(sig.len(), 128, "sig must be a full 64-byte hex Schnorr sig");
    assert!(
        sig.chars().all(|c| c.is_ascii_hexdigit()),
        "sig must be lowercase hex"
    );

    // Field-order contract (the Chirp ingest agent depends on this).
    let pos = |k: &str| json.find(k).unwrap();
    assert!(
        pos("\"id\"") < pos("\"pubkey\"")
            && pos("\"pubkey\"") < pos("\"created_at\"")
            && pos("\"created_at\"") < pos("\"kind\"")
            && pos("\"kind\"") < pos("\"tags\"")
            && pos("\"tags\"") < pos("\"content\"")
            && pos("\"content\"") < pos("\"sig\""),
        "verbatim NIP-01 field order id,pubkey,created_at,kind,tags,content,sig"
    );
}

#[test]
fn raw_tap_filters_out_non_matching_kind() {
    let slot = new_raw_event_observer_slot();
    let observer = CapturingRawObserver::new();
    // Only kind:1059 is tapped.
    register_rust_raw_observer(&slot, KindFilter::from_kinds([1059u32]), observer.clone());

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_raw_event_observers_handle(slot);

    // A kind:1 event must NOT reach a 1059-only tap.
    let v1 = signed_event_value(1, "should be filtered");
    kernel.handle_event(RelayRole::Content, "wss://relay.test", "sub-x", &v1);
    assert!(
        observer.seen.lock().unwrap().is_empty(),
        "kind:1 must be filtered out of a kind:1059-only registration"
    );

    // A kind:1059 event MUST reach it (delivered through the same
    // all-kinds seam — kind:1059 takes the `_ =>` dispatch arm, proving
    // the tap is genuinely kind-agnostic, not kind:1/6-coupled).
    let v1059 = signed_event_value(1059, "wrapped");
    kernel.handle_event(RelayRole::Content, "wss://relay.test", "sub-y", &v1059);
    let seen = observer.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, 1059);
    assert_eq!(seen[0].2.as_deref(), Some("wss://relay.test"));
}

#[test]
fn raw_tap_waits_for_store_acceptance() {
    let slot = new_raw_event_observer_slot();
    let observer = CapturingRawObserver::new();
    register_rust_raw_observer(&slot, KindFilter::from_kinds([1u32]), observer.clone());

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_raw_event_observers_handle(slot);

    let value = signed_event_value(1, "not in canonical store");
    kernel.handle_event(
        RelayRole::Content,
        "wss://relay.test",
        "untracked-sub",
        &value,
    );

    assert!(
        observer.seen.lock().unwrap().is_empty(),
        "raw tap must not fire before the store accepts the event"
    );
}

#[test]
fn raw_tap_drops_unverifiable_event() {
    let slot = new_raw_event_observer_slot();
    let observer = CapturingRawObserver::new();
    register_rust_raw_observer(&slot, KindFilter::default(), observer.clone());

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_raw_event_observers_handle(slot);

    // Take a real signed event and corrupt its signature so the kernel's
    // Schnorr gate rejects it — the tap must NOT fire (it only delivers
    // events that passed the existing sig+id gate).
    let mut bad = signed_event_value(1, "tampered");
    bad["sig"] = serde_json::Value::String("0".repeat(128));
    kernel.handle_event(RelayRole::Content, "wss://relay.test", "sub-z", &bad);
    assert!(
        observer.seen.lock().unwrap().is_empty(),
        "an event failing the Schnorr gate must never reach the raw tap"
    );
}

#[test]
fn idle_fast_path_when_no_registration() {
    // No registration at all → the kernel-side idle probe reports idle for
    // every kind, so `handle_event` skips the tap entirely (no panic, no
    // observer side-effect — pure additive no-op on the hot path).
    let slot = new_raw_event_observer_slot();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_raw_event_observers_handle(slot);
    let v = signed_event_value(1, "no listeners");
    // Must not panic; the per-kind dispatch still runs as before.
    kernel.handle_event(RelayRole::Content, "wss://relay.test", "sub-n", &v);
}
