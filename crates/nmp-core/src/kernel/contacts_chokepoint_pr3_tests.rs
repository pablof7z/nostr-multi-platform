//! ADR-0057 PR 3 acceptance tests — contacts (kind:3) are parser-fed, the
//! kernel keeps the follow-feed effects, driven by the contacts-transition
//! signal in `project_accepted_event`.
//!
//! These probe the THREE event sources that must all drive the SAME
//! kernel-owned effects through the unified chokepoint:
//!
//! 1. **Relay-delivered kind:3** → contacts cache populated + `timeline_authors`
//!    rebuilt + follow-feed interests registered.
//! 2. **Cache-served kind:3 (cold restart)** → the effects fire via the SHARED
//!    `project_accepted_event` (`feed_served_event`). Non-vacuous: the assertion
//!    that `timeline_authors` rebuilds on cache-serve fails if the contacts
//!    transition is removed from the shared helper.
//! 3. **Follow-a-new-author backfill** (the `pre_kind3_buffer`-deletion
//!    replacement) — a prior stored note of an author added to the follow set
//!    later surfaces from the store via cache-serve.
//!
//! Local read-your-writes (publish-engine → chokepoint → effects) is covered by
//! `chokepoint_tests::local_kind3_publish_updates_contacts_set`; the relay
//! fan-out onto new follows' write relays by
//! `contacts_fanout_tests`.

use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const CAROL: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn p_tag(pk: &str) -> Vec<String> {
    vec!["p".to_string(), pk.to_string()]
}

/// (1) A RELAY-delivered kind:3 for the active account drives all three
/// kernel-owned effects via the chokepoint → registered parser →
/// contacts-transition signal: the contacts cache is populated, the
/// `timeline_authors` projection is rebuilt, and the follow-feed M2 interests
/// are registered.
#[test]
fn relay_kind3_populates_cache_rebuilds_authors_and_registers_interests() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.follow_feed_kinds = std::collections::BTreeSet::from([1u32, 6u32]);
    kernel.active_account = Some(ALICE.to_string());

    // Relay delivery of ALICE's kind:3 following BOB + CAROL (through the
    // genuine ingest path: store.insert → EventIngestDispatcher → Kind3Parser →
    // contacts cache; the active-account transition fires the effects).
    kernel
        .inject_replaceable_event(
            "1111111111111111111111111111111111111111111111111111111111111111",
            ALICE,
            2_000,
            3,
            vec![p_tag(BOB), p_tag(CAROL)],
            "wss://alice.relay/",
            2_000_000,
        )
        .expect("inject kind:3 must succeed");

    // Effect (a): the capability-owned contacts cache is populated.
    assert_eq!(
        kernel.contacts_lookup().follows(ALICE),
        Some(vec![BOB.to_string(), CAROL.to_string()]),
        "the registered kind:3 parser must populate the contacts cache"
    );

    // Effect (b): the follow-derived `timeline_authors` projection is rebuilt —
    // the two follows plus the active account itself.
    let authors = kernel.timeline_authors_for_test();
    assert!(authors.contains(BOB) && authors.contains(CAROL));
    assert!(
        authors.contains(ALICE),
        "timeline_authors must include the active account itself"
    );

    // Effect (c): one M2 follow-feed interest per follow plus one for the
    // active account.
    assert_eq!(
        kernel.follow_feed_interest_ids_for_test().len(),
        3,
        "active-account kind:3 must register one follow-feed interest per follow \
         plus one for the active account"
    );
}

/// (2) A CACHE-SERVED kind:3 (cold restart) must drive the SAME kernel-owned
/// effects via the shared `project_accepted_event` (`feed_served_event`). This
/// is the cold-start scenario: the active account's kind:3 is on disk, the
/// in-memory contacts cache is empty after restart, and serving the stored
/// kind:3 must rebuild the follow set + `timeline_authors` + interests WITHOUT
/// any relay connectivity.
///
/// NON-VACUITY: the assertion that `timeline_authors` rebuilds on cache-serve
/// fails if the contacts transition is removed from `project_accepted_event` —
/// `feed_served_event` only re-projects; it does not separately drive the
/// follow-feed effects, so the shared helper is the ONLY thing that can.
#[test]
fn cache_served_kind3_drives_effects_via_shared_project_accepted_event() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.follow_feed_kinds = std::collections::BTreeSet::from([1u32, 6u32]);
    kernel.active_account = Some(ALICE.to_string());

    // Phase 1: ALICE's kind:3 (following BOB) lands and persists to the store.
    kernel
        .inject_replaceable_event(
            "2222222222222222222222222222222222222222222222222222222222222222",
            ALICE,
            2_000,
            3,
            vec![p_tag(BOB)],
            "wss://alice.relay/",
            2_000_000,
        )
        .expect("inject kind:3 must succeed");
    assert!(
        kernel.timeline_authors_for_test().contains(BOB),
        "precondition: BOB is a follow-derived timeline author after the live ingest"
    );

    // Phase 2: cold restart — the in-memory contacts cache is lost (production
    // rebuilds it from the store via cache-serve). Also clear the follow-feed
    // derived state so we can prove cache-serve rebuilds it.
    kernel.clear_test_contacts_cache();
    kernel.sync_follow_feed_interests(&[]); // withdraw + clear timeline_authors
    kernel.clear_served_interest_shapes();
    assert!(
        kernel.contacts_lookup().follows(ALICE).is_none(),
        "precondition: contacts cache empty after cold restart"
    );
    assert!(
        !kernel.timeline_authors_for_test().contains(BOB),
        "precondition: timeline_authors cleared after cold restart"
    );

    // Phase 3: cache-serve the active account's stored kind:3 (the discovery /
    // account-profile interest covers kind:3). This reads the store and routes
    // each served event through `feed_served_event` → the SHARED
    // `project_accepted_event` → the registered kind:3 parser → contacts cache →
    // the active-account contacts-transition signal → the follow-feed effects.
    let shape = crate::planner::InterestShape {
        authors: std::collections::BTreeSet::from([ALICE.to_string()]),
        kinds: std::collections::BTreeSet::from([3u32]),
        ..Default::default()
    };
    let sub_key = crate::subs::SubKey::new("cold-restart-contacts-serve");
    kernel.enqueue_interest_cache_serve(&sub_key, &shape);

    // The cache-served kind:3 rebuilt the follow set + the kernel-owned effects.
    assert_eq!(
        kernel.contacts_lookup().follows(ALICE),
        Some(vec![BOB.to_string()]),
        "cache-serve must repopulate the contacts cache via the registered parser"
    );
    assert!(
        kernel.timeline_authors_for_test().contains(BOB),
        "NON-VACUITY: cache-serve must rebuild timeline_authors via the contacts \
         transition in the SHARED project_accepted_event (feed_served_event only \
         re-projects — the shared helper is the only thing that drives the effect)"
    );
    assert!(
        !kernel.follow_feed_interest_ids_for_test().is_empty(),
        "cache-serve must re-register the follow-feed M2 interests"
    );
}

/// (2b) Cold-restart offline rehydration via the UNIVERSAL store-first bootstrap
/// (ADR-0045 R3). On relaunch the in-memory contacts cache is empty and the
/// active account's own kind:3 is the INPUT that defines the follow-feed interest
/// — never a member of any consumer interest — so nothing in the consumer path
/// would ever serve it. The fix is universal store-first: the bootstrap
/// self-kinds interest (kinds 0/3/10002/…) registered by
/// `active_account_bootstrap_requests` is itself cache-served on open, so the
/// stored kind:3 flows through the SHARED `project_accepted_event` → `Kind3Parser`
/// → contacts cache → contacts-transition → follow-feed registration — with NO
/// relay connectivity.
///
/// NON-VACUITY: drop the `enqueue_interest_cache_serve` call from
/// `register_tailing_self_kinds_interest` and this fails — the bootstrap interest
/// would only REQ the network and the offline cache would stay empty.
#[test]
fn cold_restart_bootstrap_self_kinds_interest_cache_serves_stored_kind3() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.follow_feed_kinds = std::collections::BTreeSet::from([1u32, 6u32]);
    kernel.active_account = Some(ALICE.to_string());

    // Phase 1: ALICE's kind:3 (following BOB + CAROL) lands and persists.
    kernel
        .inject_replaceable_event(
            "4444444444444444444444444444444444444444444444444444444444444444",
            ALICE,
            2_000,
            3,
            vec![p_tag(BOB), p_tag(CAROL)],
            "wss://alice.relay/",
            2_000_000,
        )
        .expect("inject kind:3 must succeed");

    // Phase 2: cold restart — lose the in-memory contacts cache + the
    // follow-feed derived state + served-interest completion set. The kind:3
    // itself remains in the event store.
    kernel.clear_test_contacts_cache();
    kernel.sync_follow_feed_interests(&[]);
    kernel.clear_served_interest_shapes();
    assert!(
        kernel.contacts_lookup().follows(ALICE).is_none(),
        "precondition: contacts cache empty after cold restart"
    );
    assert!(
        !kernel.timeline_authors_for_test().contains(BOB),
        "precondition: timeline_authors cleared after cold restart"
    );

    // Phase 3: the cold-start bootstrap path — this is what `start()` /
    // identity-restore runs. NO relay connectivity; the store-first half of the
    // bootstrap self-kinds interest serves the stored kind:3.
    let _ = kernel.active_account_bootstrap_requests();

    // The stored kind:3 was cache-served through the shared chokepoint,
    // rebuilding the follow set + timeline_authors + interests from disk alone.
    assert_eq!(
        kernel.contacts_lookup().follows(ALICE),
        Some(vec![BOB.to_string(), CAROL.to_string()]),
        "store-first bootstrap must repopulate the contacts cache from the stored \
         kind:3 with no relay connectivity"
    );
    assert!(
        kernel.timeline_authors_for_test().contains(BOB)
            && kernel.timeline_authors_for_test().contains(CAROL),
        "store-first bootstrap must rebuild timeline_authors from the rehydrated \
         follow set"
    );
    assert_eq!(
        kernel.follow_feed_interest_ids_for_test().len(),
        3,
        "store-first bootstrap must register one follow-feed interest per follow \
         plus the active account"
    );
}

/// (3) Follow-a-new-author backfill — the `pre_kind3_buffer`-deletion
/// replacement (ADR-0057). A note from CAROL arrives and persists BEFORE ALICE
/// follows her (admission ≠ persistence: it is stored on valid signature, not
/// parked). When ALICE's kind:3 later adds CAROL, `sync_follow_feed_interests`
/// enqueues a cache-serve that surfaces CAROL's prior stored note into the
/// timeline read-cache — no buffer, no replay queue.
#[test]
fn follow_new_author_backfills_prior_stored_note_from_store() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.follow_feed_kinds = std::collections::BTreeSet::from([1u32]);
    kernel.active_account = Some(ALICE.to_string());

    // CAROL's note arrives + persists BEFORE she is followed (no pre_kind3
    // parking — it is stored on valid signature alone). Delivered on a non-
    // follow-feed sub so the live timeline projection does not append it yet.
    let carol_keys = ::nostr::Keys::generate();
    let carol = carol_keys.public_key().to_hex();
    let note = {
        use ::nostr::{EventBuilder, Timestamp};
        let ev = EventBuilder::text_note("carol's pre-follow note")
            .custom_created_at(Timestamp::from(1_700_000_000u64))
            .sign_with_keys(&carol_keys)
            .expect("sign");
        crate::kernel::nostr::NostrEvent {
            id: ev.id.to_hex(),
            pubkey: ev.pubkey.to_hex(),
            created_at: ev.created_at.as_secs(),
            kind: 1,
            tags: Vec::new(),
            content: ev.content.clone(),
            sig: ev.sig.to_string(),
        }
    };
    let note_id = note.id.clone();
    kernel.ingest_accepted_event(
        crate::kernel::ingest::IngestSource::Relay {
            relay_url: "wss://firehose.relay/",
            sub_id: "some-other-sub",
        },
        note,
    );
    assert!(
        !kernel.timeline.iter().any(|id| id == &note_id),
        "precondition: CAROL's note is stored but NOT yet in the timeline \
         (she is not followed)"
    );

    // ALICE now follows CAROL (relay kind:3). The active-account contacts
    // transition drives `sync_follow_feed_interests`, which enqueues a
    // cache-serve for CAROL's interest and drains one chunk — surfacing her
    // prior stored note into the timeline read-cache.
    kernel
        .inject_replaceable_event(
            "3333333333333333333333333333333333333333333333333333333333333333",
            ALICE,
            2_000,
            3,
            vec![p_tag(&carol)],
            "wss://alice.relay/",
            2_000_000,
        )
        .expect("inject kind:3 must succeed");

    assert!(
        kernel.timeline.iter().any(|id| id == &note_id),
        "backfill: following CAROL must surface her prior stored note from the \
         store via cache-serve (the pre_kind3_buffer-deletion replacement)"
    );
}

/// (4) Sign-in seed — `prepopulate_contacts` restores KNOWN contacts. It is NOT
/// a newly-ingested event, so it must NOT emit a phantom kind:3 to app
/// `KernelEventObserver`s (which would double up with the real signed kind:3
/// that arrives later, and surface a fake, non-persisted event). It must still
/// drive the kernel-owned follow-feed effects directly.
///
/// This is the BLOCKER fix: prepopulate writes the cache + fires effects via the
/// `ContactsLookup` writer + `on_active_contacts_changed` — WITHOUT the
/// `project_accepted_event` observer fan-out and WITHOUT a fabricated id/sig.
#[test]
fn prepopulate_contacts_seeds_effects_without_emitting_a_phantom_event() {
    use crate::actor::{new_event_observer_slot, register_rust_observer, KernelEventObserver};
    use crate::substrate::KernelEvent;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    struct CapturingObserver {
        count: AtomicU32,
        last_kind: Mutex<Option<u32>>,
    }
    impl KernelEventObserver for CapturingObserver {
        fn on_kernel_event(&self, event: &KernelEvent) {
            self.count.fetch_add(1, Ordering::SeqCst);
            if let Ok(mut g) = self.last_kind.lock() {
                *g = Some(event.kind);
            }
        }
    }

    let slot = new_event_observer_slot();
    let observer = Arc::new(CapturingObserver {
        count: AtomicU32::new(0),
        last_kind: Mutex::new(None),
    });
    register_rust_observer(&slot, observer.clone());

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_event_observers_handle(slot);
    kernel.follow_feed_kinds = std::collections::BTreeSet::from([1u32, 6u32]);
    kernel.active_account = Some(ALICE.to_string());

    // Sign-in seed: restore the account's known follows (BOB, CAROL).
    kernel.prepopulate_contacts(ALICE.to_string(), vec![BOB.to_string(), CAROL.to_string()]);

    // NO phantom event: prepopulate must NOT reach the observer fan-out.
    assert_eq!(
        observer.count.load(Ordering::SeqCst),
        0,
        "prepopulate_contacts must NOT emit a (fake, non-persisted) kind:3 to \
         KernelEventObservers — it is a sign-in cache seed, not an ingested event \
         (last observed kind = {:?})",
        observer.last_kind.lock().unwrap()
    );

    // But the kernel-owned effects ARE driven directly: the cache is populated,
    // `timeline_authors` is rebuilt, and the follow-feed interests are registered.
    assert_eq!(
        kernel.contacts_lookup().follows(ALICE),
        Some(vec![BOB.to_string(), CAROL.to_string()]),
        "prepopulate must write the contacts cache via the ContactsLookup writer"
    );
    let authors = kernel.timeline_authors_for_test();
    assert!(
        authors.contains(BOB) && authors.contains(CAROL) && authors.contains(ALICE),
        "prepopulate must rebuild timeline_authors via on_active_contacts_changed"
    );
    assert_eq!(
        kernel.follow_feed_interest_ids_for_test().len(),
        3,
        "prepopulate must register the follow-feed interests (2 follows + the active account)"
    );
}
