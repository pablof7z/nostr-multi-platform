//! F-04 wildcard-arm regression coverage.
//!
//! The explicit kind:9735 arm in `handle_event` is already exercised by
//! `kernel/raw_event_observer_tests.rs` (raw-tap path) and the
//! `ZapsAggregateProjection` integration tests. These tests focus on the
//! `_ =>` wildcard arm, which prior to this fix called only
//! `verify_and_persist` and therefore never fanned the store-accepted
//! `KernelEvent` out to `KernelEventObserver`s. The structural consequence
//! was that NIP-29-style projections (kinds 9, 11, 1111, 39000/1/2) and
//! gift-wraps (kind:1059) — none of which have an explicit arm — were
//! structurally deaf to events the store had already canonicalized.
//!
//! Both tests drive REAL Schnorr-signed events through `handle_event`
//! (the production all-kinds entry point) because `verify_and_persist`
//! runs full secp256k1 verification — a fake-hex fixture is dropped
//! before reaching the store and the wildcard arm. The signing pattern
//! mirrors `kernel/raw_event_observer_tests.rs::signed_event_value` and
//! `kernel/ingest_tests.rs::signed_note`.
use super::*;
use crate::actor::{new_event_observer_slot, register_rust_observer, KernelEventObserver};
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::substrate::KernelEvent;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

/// Test double that records every observer notification it receives so
/// the assertions can check count + kind. Mirrors
/// `kernel/event_observer_tests.rs::CapturingObserver`.
struct CountingObserver {
    count: AtomicU32,
    kinds: Mutex<Vec<u32>>,
}

impl CountingObserver {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            count: AtomicU32::new(0),
            kinds: Mutex::new(Vec::new()),
        })
    }
}

impl KernelEventObserver for CountingObserver {
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.count.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut guard) = self.kinds.lock() {
            guard.push(event.kind);
        }
    }
}

/// Build a real Schnorr-signed event of `kind` and return its NIP-01 JSON
/// `Value` (the exact shape `handle_event` parses off the wire). Reuses
/// the proven pattern from `kernel/raw_event_observer_tests.rs` so the
/// fixture survives `VerifiedEvent::try_from_raw`'s real sig verification.
fn signed_event_value(kind: u32, content: &str) -> serde_json::Value {
    use nostr::{EventBuilder, Keys, Kind};
    let keys = Keys::generate();
    let nostr_event = EventBuilder::new(Kind::from(kind as u16), content)
        .sign_with_keys(&keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
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

/// A kind that hits the `_ =>` wildcard arm (NIP-29 chat message kind:9)
/// fans out to registered `KernelEventObserver`s after a successful
/// store insert. Before the F-04 wildcard-arm fix this assertion failed
/// because the arm called only `verify_and_persist` and never
/// `notify_event_observers`.
#[test]
fn wildcard_kind_fan_out_to_event_observers() {
    let slot = new_event_observer_slot();
    let observer = CountingObserver::new();
    register_rust_observer(&slot, observer.clone());

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_event_observers_handle(slot);

    // Kind:9 (NIP-29 group chat message) — hits the wildcard arm because
    // no explicit match arm above lists it. `GroupChatProjection` in
    // `apps/chirp/nmp-app-chirp/src/ffi/register.rs` is registered as a
    // `KernelEventObserver` for exactly this kind.
    let value = signed_event_value(9, "hello group");
    kernel.handle_event(
        RelayRole::Content,
        "wss://relay.test",
        "group-chat-sub",
        &value,
    );

    assert_eq!(
        observer.count.load(Ordering::SeqCst),
        1,
        "wildcard-arm kinds must fan out to KernelEventObservers exactly \
         once after a successful store insert"
    );
    let kinds = observer.kinds.lock().unwrap().clone();
    assert_eq!(
        kinds,
        vec![9],
        "the observer must receive the event with its original kind \
         (kind:9 NIP-29 group chat)"
    );
}

/// Kind:0 is handled by an explicit arm because it updates the kernel's
/// profile cache. It must still fan the accepted event to observers so app
/// projections such as Chirp's modular timeline can refresh author display
/// names from kind:0 without app-local fetching or polling.
#[test]
fn kind0_fans_out_to_event_observers() {
    let slot = new_event_observer_slot();
    let observer = CountingObserver::new();
    register_rust_observer(&slot, observer.clone());

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_event_observers_handle(slot);

    let value = signed_event_value(0, r#"{"display_name":"Alice"}"#);
    kernel.handle_event(
        RelayRole::Indexer,
        "wss://relay.test",
        "profile-claim-1-test",
        &value,
    );

    assert_eq!(
        observer.count.load(Ordering::SeqCst),
        1,
        "accepted kind:0 metadata must fan out to KernelEventObservers"
    );
    let kinds = observer.kinds.lock().unwrap().clone();
    assert_eq!(kinds, vec![0]);
}

/// Kind:3 is also an explicit arm. The NIP-02 follow-list projection consumes
/// it through KernelEventObserver, so accepted contact lists must fan out just
/// like timeline, wildcard, and kind:0 events.
#[test]
fn kind3_fans_out_to_event_observers() {
    let slot = new_event_observer_slot();
    let observer = CountingObserver::new();
    register_rust_observer(&slot, observer.clone());

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_event_observers_handle(slot);

    let value = signed_event_value(3, "");
    kernel.handle_event(
        RelayRole::Indexer,
        "wss://relay.test",
        "account-profile",
        &value,
    );

    assert_eq!(
        observer.count.load(Ordering::SeqCst),
        1,
        "accepted kind:3 contact lists must fan out to KernelEventObservers"
    );
    let kinds = observer.kinds.lock().unwrap().clone();
    assert_eq!(kinds, vec![3]);
}

/// D4 duplicate dedup: a second delivery of the same event id must NOT
/// re-fire the observer. The store returns `Duplicate` on the second
/// insert, which falls outside the `Inserted | Replaced` match in the
/// wildcard arm, so the fan-out is skipped. This is the exact protection
/// that mirrors the explicit kind:9735 arm.
#[test]
fn wildcard_kind_dedup_no_double_fan_out() {
    let slot = new_event_observer_slot();
    let observer = CountingObserver::new();
    register_rust_observer(&slot, observer.clone());

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_event_observers_handle(slot);

    let value = signed_event_value(9, "duplicate event");
    // First delivery — store returns Inserted, observer fires once.
    kernel.handle_event(
        RelayRole::Content,
        "wss://relay.a.test",
        "group-chat-sub",
        &value,
    );
    // Second delivery of the SAME event id (a sibling relay re-delivering
    // a message we already have). The store returns Duplicate and the
    // wildcard arm's `Inserted | Replaced` gate skips the fan-out.
    kernel.handle_event(
        RelayRole::Content,
        "wss://relay.b.test",
        "group-chat-sub",
        &value,
    );

    assert_eq!(
        observer.count.load(Ordering::SeqCst),
        1,
        "duplicate sibling-relay deliveries of the same event id must NOT \
         double-fire KernelEventObservers (D4)"
    );
}
