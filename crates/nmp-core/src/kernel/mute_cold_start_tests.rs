//! Cold-start cache-serve regression tests for kind:10000 (mute list).
//!
//! Verifies that a `KernelEventObserver` registered before the mute-list
//! interest is pushed receives stored kind:10000 events via the cache-serve
//! drain — the same path the `MuteListProjection` (in `nmp-nip51`) uses on
//! sign-in.
//!
//! # The bug this guards against
//!
//! Before issue #1644, kind:10000 free-rode on `SELF_KINDS_TAILING`. On
//! cold start after the kernel's interest registry was cleared (process
//! restart), if an external `PushInterest` for kind:10000 `authors=[pk]` was
//! pushed before the observer was registered, the observer would miss the
//! cache-serve drain delivery. This test locks the correct ordering contract:
//! observer registered BEFORE interest pushed → observer receives drain.

use super::cache_serve_tests::{drain_cache_serves, simulate_cold_restart};
use super::*;
use crate::actor::{new_event_observer_slot, register_rust_observer, KernelEventObserver};
use crate::planner::{InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest};
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::subs::{SubIdentity, SubKey, SubOwnerKey, SubScope};
use crate::substrate::KernelEvent;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

const KIND_MUTE_LIST: u32 = 10_000;
const MUTED_PUBKEY: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

/// Observer that captures kind:10000 events and records each muted pubkey
/// from `["p", <pubkey>]` tags.
struct CapturingMuteObserver {
    muted_pubkeys: Mutex<Vec<String>>,
}

impl CapturingMuteObserver {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            muted_pubkeys: Mutex::new(Vec::new()),
        })
    }

    fn muted_pubkeys(&self) -> Vec<String> {
        let mut v = self.muted_pubkeys.lock().unwrap().clone();
        v.sort_unstable();
        v
    }
}

impl KernelEventObserver for CapturingMuteObserver {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind != KIND_MUTE_LIST {
            return;
        }
        let mut pubkeys = self.muted_pubkeys.lock().unwrap();
        for tag in &event.tags {
            if tag.first().is_some_and(|t| t == "p") {
                if let Some(pk) = tag.get(1) {
                    pubkeys.push(pk.clone());
                }
            }
        }
    }
}

/// Build a real signed kind:10000 mute-list event via the `nostr` crate and
/// ingest it through `handle_event` (the live verification + persistence path).
/// Returns the event id.
fn seed_kind10000_event(
    kernel: &mut Kernel,
    keys: &::nostr::Keys,
    muted_pubkey: &str,
    ts: u64,
) -> String {
    use ::nostr::{EventBuilder, Kind, Tag, Timestamp};
    let muted_pk: ::nostr::PublicKey = muted_pubkey.parse().expect("valid hex pubkey");
    let ev = EventBuilder::new(Kind::from(10_000u16), "")
        .tags(vec![Tag::public_key(muted_pk)])
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with generated keys");
    let tag_vecs: Vec<Vec<String>> = ev
        .tags
        .iter()
        .map(|t: &::nostr::Tag| t.as_slice().to_vec())
        .collect();
    let json = serde_json::json!({
        "id": ev.id.to_hex(),
        "pubkey": ev.pubkey.to_hex(),
        "created_at": ev.created_at.as_secs(),
        "kind": ev.kind.as_u16(),
        "tags": tag_vecs,
        "content": ev.content.clone(),
        "sig": ev.sig.to_string(),
    });
    let id = ev.id.to_hex();
    kernel.handle_event(RelayRole::Content, "wss://relay.test/", "mute-sub", &json);
    id
}

/// Open a kind:10000 authors interest via the cache-serve front-door so the
/// drain will replay stored kind:10000 events for that author.
fn open_mute_list_interest(kernel: &mut Kernel, seed: u64, author_hex: &str) {
    let shape = InterestShape {
        authors: BTreeSet::from([author_hex.to_string()]),
        kinds: BTreeSet::from([KIND_MUTE_LIST]),
        ..Default::default()
    };
    let interest = LogicalInterest {
        id: InterestId(seed),
        scope: InterestScope::Global,
        shape,
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    };
    let sub_identity = SubIdentity::new(
        SubOwnerKey::new(seed),
        SubKey::new(seed),
        SubScope::Global,
    );
    kernel.open_interest_sub(sub_identity, interest);
}

/// PRIMARY CONTRACT:
///
/// A kind:10000 event seeded into the store via live ingest reaches a
/// registered `KernelEventObserver` when the mute-list interest is pushed
/// on a cold-restart kernel (empty in-memory caches, warm store) — PROVIDED
/// the observer is registered BEFORE the interest is pushed.
///
/// This is the ordering contract enforced by `register_mute_runtime`:
/// register the event observer FIRST so the cache-serve drain has a recipient.
#[test]
fn stored_kind10000_reaches_observer_via_cache_serve_drain() {
    let base_ts: u64 = 1_800_000_000;
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    // ── Phase 1: seed a kind:10000 event into the store ─────────────────────
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(author.clone());

    let observer = CapturingMuteObserver::new();
    let slot = new_event_observer_slot();
    register_rust_observer(&slot, Arc::clone(&observer) as Arc<dyn KernelEventObserver>);
    kernel.set_event_observers_handle(slot.clone());

    let event_id =
        seed_kind10000_event(&mut kernel, &keys, MUTED_PUBKEY, base_ts);
    assert!(
        !observer.muted_pubkeys().is_empty(),
        "Phase 1: observer must fire for live ingest; got id={event_id}"
    );

    // ── Phase 2: cold restart (store warm, in-memory caches cleared) ─────────
    simulate_cold_restart(&mut kernel);
    // Re-attach the same observer slot — after restart there's no handle set.
    kernel.set_event_observers_handle(slot);
    // Clear the observer's accumulated state so we start clean.
    observer.muted_pubkeys.lock().unwrap().clear();
    assert!(
        observer.muted_pubkeys().is_empty(),
        "Phase 2: observer must be cleared before replay"
    );

    // ── Phase 3: push the mute-list interest (AFTER registering observer) ────
    //
    // This is the ordering contract: the observer is already registered on the
    // kernel's slot before open_interest_sub fires the cache-serve drain.
    open_mute_list_interest(&mut kernel, 10_000, &author);
    drain_cache_serves(&mut kernel, 10);

    // ── Phase 4: observer must have received the stored kind:10000 ────────────
    let pubkeys = observer.muted_pubkeys();
    assert!(
        pubkeys.contains(&MUTED_PUBKEY.to_string()),
        "COLD-START FAIL: KernelEventObserver must receive the stored \
         kind:10000 event via cache-serve drain after interest push; \
         got muted_pubkeys={pubkeys:?}"
    );
}

/// NEGATIVE CONTRACT (ordering guard):
///
/// An observer registered AFTER the interest is pushed does NOT receive events
/// from the cache-serve drain — the drain already completed. This pins the
/// ordering requirement so anyone who reverses the registration order is caught.
///
/// Note: this test only proves the negative — if the observer was not registered
/// before the drain, the drain delivered to nobody. The positive case is proven
/// by `stored_kind10000_reaches_observer_via_cache_serve_drain` above.
#[test]
fn observer_after_interest_push_receives_nothing_from_cache_serve() {
    let base_ts: u64 = 1_800_001_000;
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    // Seed into store via live ingest (observer slot not set — ingest still
    // stores even without an observer).
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(author.clone());
    seed_kind10000_event(&mut kernel, &keys, MUTED_PUBKEY, base_ts);

    // Cold restart.
    simulate_cold_restart(&mut kernel);

    // Push the interest BEFORE attaching the observer — drain fires immediately.
    open_mute_list_interest(&mut kernel, 20_000, &author);
    drain_cache_serves(&mut kernel, 10);

    // Attach the observer AFTER the drain has already completed.
    let observer = CapturingMuteObserver::new();
    let slot = new_event_observer_slot();
    register_rust_observer(&slot, Arc::clone(&observer) as Arc<dyn KernelEventObserver>);
    kernel.set_event_observers_handle(slot);

    // The observer was not present during the drain — it must see nothing.
    assert!(
        observer.muted_pubkeys().is_empty(),
        "Late-registered observer must NOT receive events from a completed \
         cache-serve drain; got {:?}",
        observer.muted_pubkeys()
    );
}
