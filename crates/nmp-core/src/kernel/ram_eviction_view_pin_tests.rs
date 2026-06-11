//! #1088 / PR #1096 review — open-interest pin invariant tests.
//!
//! V-112 (ADR-0042) deleted the legacy `AuthorViewState`/`ThreadViewState`
//! stack; open views are now per-app FlatFeeds backed by the generic
//! `open_interest` seam.  `open_interest` registers a refcounted
//! `LogicalInterest` in the planner registry and writes NOTHING to
//! `event_claims` — claims are the embed mechanism, interests are a separate
//! stack.  The feed-engine read path reads `self.events` / `self.profiles`
//! with NO store fallback, so RAM eviction must pin the open-interest working
//! set directly (derived from `lifecycle.registry().iter_active()` in
//! `Kernel::open_view_pins`, using the same `matches_event_with_id`
//! predicate `should_store_event`'s admission clause uses).
//!
//! Each test makes the pinned entries the OLDEST in the map (lowest
//! `created_at` → first eviction candidates without the pin) so the
//! assertions are sharp.
//!
//! Shared fixtures live in `ram_eviction_tests` (`pub(super)` helpers).

use super::ram_eviction::{EVENTS_RAM_HWM, PROFILES_RAM_HWM};
use super::ram_eviction_tests::{
    inject_events, inject_profiles, make_pubkey, pin_clock, T0_SECS,
};
use super::*;
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::store::{RawEvent, VerifiedEvent};

/// Register a generic `open_interest` on the kernel from a verbatim NIP-01
/// filter — the exact body of the `ActorCommand::OpenInterest` dispatch arm
/// (`actor/dispatch.rs::build_open_interest` + `Kernel::open_interest_sub`),
/// reproduced here because the dispatch helper is private to the actor
/// module and these tests exercise the kernel-level pin invariant directly.
fn open_interest(kernel: &mut Kernel, filter_json: &str, consumer_id: &str) {
    use crate::planner::{InterestLifecycle, InterestScope, LogicalInterest};
    use crate::subs::sub_key::{SubIdentity, SubKey, SubOwnerKey, SubScope};

    let shape = crate::planner::InterestShape::from_filter_json(filter_json)
        .expect("test filter must be a valid NIP-01 filter object");
    let key = SubKey::builder("open-interest").with(&shape).with(1u32).finish();
    let identity = SubIdentity::new(SubOwnerKey::new(consumer_id), key, SubScope::Global);
    let interest = LogicalInterest {
        scope: InterestScope::Global,
        shape,
        lifecycle: InterestLifecycle::Tailing,
        ..LogicalInterest::default()
    };
    let _ = kernel.open_interest_sub(identity, interest);
}

/// Inject one kind:1 event with explicit NIP-10 `e` tags through the real
/// test ingest path.  Used to build thread structures (root + replies).
fn inject_tagged_note(
    kernel: &mut Kernel,
    id: &str,
    pubkey: &str,
    created_at: u64,
    tags: Vec<Vec<String>>,
) {
    let raw = RawEvent {
        id: id.to_string(),
        pubkey: pubkey.to_string(),
        created_at,
        kind: 1,
        tags,
        content: format!("thread note {id}"),
        sig: "a".repeat(128),
    };
    let verified = VerifiedEvent::from_raw_unchecked(raw);
    kernel.ingest_pre_verified_event(RelayRole::Content, "", verified);
}

/// An OPEN thread feed's root + focused + reply events must survive eviction
/// even though `open_interest` writes nothing to `event_claims`.
///
/// The thread feed is composed exactly the way an app-side thread FlatFeed
/// composes its hydration (ADR-0042): one `ids` interest for the root +
/// focused (+ hydrated-ancestor) notes, one `#e` interest for the replies.
///
/// The thread events are deliberately the OLDEST entries (lowest
/// `created_at`) so that, without the open-interest pin, they would be the
/// very first eviction candidates — making the assertion sharp.
#[test]
fn open_thread_interest_events_survive_eviction() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS);

    // Thread structure (oldest events in the map):
    //   root R  (no tags)
    //   focused F  -- e-tag -> R
    //   replies X1..X5  -- e-tag -> R
    let root_id = format!("{:0>64x}", 0xA00001u64);
    let root_author = make_pubkey(5_001);
    inject_tagged_note(&mut kernel, &root_id, &root_author, T0_SECS, vec![]);

    let focused_id = format!("{:0>64x}", 0xA00002u64);
    let focused_author = make_pubkey(5_002);
    inject_tagged_note(
        &mut kernel,
        &focused_id,
        &focused_author,
        T0_SECS + 1,
        vec![vec!["e".to_string(), root_id.clone()]],
    );

    let mut reply_ids = Vec::new();
    for n in 0..5u64 {
        let reply_id = format!("{:0>64x}", 0xA00010 + n);
        let reply_author = make_pubkey(5_010 + n as usize);
        inject_tagged_note(
            &mut kernel,
            &reply_id,
            &reply_author,
            T0_SECS + 2 + n,
            vec![vec!["e".to_string(), root_id.clone()]],
        );
        reply_ids.push(reply_id);
    }

    // A hydration-fetched ancestor id, cached in `self.events`.  App-side
    // thread hydration keeps it in the `ids` interest while the view stays
    // open; without the pin, eviction would remove it from under the open
    // feed (the read path has no store fallback).
    let hydrated_id = format!("{:0>64x}", 0xA00099u64);
    inject_tagged_note(&mut kernel, &hydrated_id, &make_pubkey(5_099), T0_SECS + 7, vec![]);

    // Open the thread through the REAL generic seam: an `ids` interest for
    // the root/focused/hydrated notes + a `#e` interest for the replies.
    open_interest(
        &mut kernel,
        &format!(r#"{{"ids":["{root_id}","{focused_id}","{hydrated_id}"]}}"#),
        "thread-feed-test",
    );
    open_interest(
        &mut kernel,
        &format!(r##"{{"kinds":[1],"#e":["{root_id}"]}}"##),
        "thread-feed-test",
    );

    // Flood with NEWER unrelated events to push the map over the HWM.
    let over = EVENTS_RAM_HWM + 74;
    inject_events(&mut kernel, over, T0_SECS + 10_000);

    assert!(
        kernel.events.len() > EVENTS_RAM_HWM,
        "precondition: must exceed HWM (len={})",
        kernel.events.len()
    );

    kernel.evict_ram_caches();

    assert!(
        kernel.events.len() <= EVENTS_RAM_HWM,
        "cap must hold (len={})",
        kernel.events.len()
    );
    for id in std::iter::once(&root_id)
        .chain(std::iter::once(&focused_id))
        .chain(reply_ids.iter())
        .chain(std::iter::once(&hydrated_id))
    {
        assert!(
            kernel.events.contains_key(id),
            "open-thread-interest event {id} must survive eviction"
        );
    }
}

/// An OPEN author feed's notes (a NON-followed author — not in
/// `timeline_authors`, not in `timeline`) must survive eviction while the
/// `authors` interest stays registered.
#[test]
fn open_author_interest_events_survive_eviction() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS);

    // 10 oldest notes by a non-followed author.
    let author = make_pubkey(6_001);
    let mut note_ids = Vec::new();
    for n in 0..10u64 {
        let id = format!("{:0>64x}", 0xB00000 + n);
        inject_tagged_note(&mut kernel, &id, &author, T0_SECS + n, vec![]);
        note_ids.push(id);
    }
    assert!(
        !kernel.timeline_authors.contains(&author),
        "precondition: author must NOT be followed"
    );

    // Open the author feed through the REAL generic seam.
    open_interest(
        &mut kernel,
        &format!(r#"{{"kinds":[1],"authors":["{author}"]}}"#),
        "author-feed-test",
    );

    // Flood with newer unrelated events.
    let over = EVENTS_RAM_HWM + 74;
    inject_events(&mut kernel, over, T0_SECS + 10_000);

    kernel.evict_ram_caches();

    assert!(
        kernel.events.len() <= EVENTS_RAM_HWM,
        "cap must hold (len={})",
        kernel.events.len()
    );
    for id in &note_ids {
        assert!(
            kernel.events.contains_key(id),
            "open-author-interest note {id} must survive eviction"
        );
    }
}

/// The OPEN author feed's profile (a non-followed, non-claimed author whose
/// notes are pinned by the interest) must survive profile eviction —
/// `profile_for_pubkey()` has no store fallback.
#[test]
fn open_author_interest_profile_survives_eviction() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS);

    // Flood profiles over the HWM; the FIRST injected profile (oldest
    // created_at → first eviction candidate) is the author we then open.
    let over = PROFILES_RAM_HWM + 74;
    let pubkeys = inject_profiles(&mut kernel, over, T0_SECS);
    let viewed_author = pubkeys[0].clone();

    // One cached note by the viewed author — the open feed renders it, and
    // its author pubkey is what pins the profile (`open_view_pins` derives
    // profile pins from the pinned events' authors).
    let note_id = format!("{:0>64x}", 0xB10000u64);
    inject_tagged_note(&mut kernel, &note_id, &viewed_author, T0_SECS, vec![]);

    open_interest(
        &mut kernel,
        &format!(r#"{{"kinds":[1],"authors":["{viewed_author}"]}}"#),
        "author-feed-test",
    );

    kernel.evict_ram_caches();

    assert!(
        kernel.profiles.len() <= PROFILES_RAM_HWM,
        "cap must hold (len={})",
        kernel.profiles.len()
    );
    assert!(
        kernel.profiles.contains_key(&viewed_author),
        "open author feed's profile must survive eviction"
    );
}

/// Thread PARTICIPANT profiles (authors of the open thread feed's events)
/// must survive profile eviction — they feed `timeline_item()` enrichment
/// for the open feed via `profile_for_pubkey()`.
#[test]
fn open_thread_interest_participant_profiles_survive_eviction() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS);

    // Flood profiles over the HWM first; the two OLDEST profiles belong to
    // the thread participants (root + reply authors).
    let over = PROFILES_RAM_HWM + 74;
    let pubkeys = inject_profiles(&mut kernel, over, T0_SECS);
    let root_author = pubkeys[0].clone();
    let reply_author = pubkeys[1].clone();

    let root_id = format!("{:0>64x}", 0xC00001u64);
    inject_tagged_note(&mut kernel, &root_id, &root_author, T0_SECS, vec![]);
    let reply_id = format!("{:0>64x}", 0xC00002u64);
    inject_tagged_note(
        &mut kernel,
        &reply_id,
        &reply_author,
        T0_SECS + 1,
        vec![vec!["e".to_string(), root_id.clone()]],
    );

    open_interest(
        &mut kernel,
        &format!(r#"{{"ids":["{root_id}"]}}"#),
        "thread-feed-test",
    );
    open_interest(
        &mut kernel,
        &format!(r##"{{"kinds":[1],"#e":["{root_id}"]}}"##),
        "thread-feed-test",
    );

    kernel.evict_ram_caches();

    assert!(
        kernel.profiles.len() <= PROFILES_RAM_HWM,
        "cap must hold (len={})",
        kernel.profiles.len()
    );
    assert!(
        kernel.profiles.contains_key(&root_author),
        "thread root author's profile must survive eviction"
    );
    assert!(
        kernel.profiles.contains_key(&reply_author),
        "thread reply author's profile must survive eviction"
    );
}
