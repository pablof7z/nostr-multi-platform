//! #1088 / PR #1096 review — open-view pin invariant tests.
//!
//! `open_thread` / `open_author` set refcounted `ViewInterest`s
//! (`thread_view.selected_thread` / `author_view.selected_author`) and write
//! NOTHING to `event_claims` — claims are the embed mechanism, views are a
//! separate stack.  `thread_items()` / `author_items()` /
//! `profile_for_pubkey()` read `self.events` / `self.profiles` with NO store
//! fallback, so RAM eviction must pin the open-view working set directly
//! (derived from live view state in `Kernel::open_view_pins`).
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

/// An OPEN thread view's root + focused + reply events must survive eviction
/// even though `open_thread` writes nothing to `event_claims`, and
/// `thread_items()` must still return the full set afterwards.
///
/// The thread events are deliberately the OLDEST entries (lowest
/// `created_at`) so that, without the open-view pin, they would be the very
/// first eviction candidates — making the assertion sharp.
#[test]
fn open_thread_view_events_survive_eviction() {
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

    // Open the thread through the REAL view path (refcounted ViewInterest;
    // can_send=false defers the wire requests — irrelevant here).
    kernel.open_thread(
        focused_id.clone(),
        std::collections::BTreeSet::from([1u32]),
        false,
    );

    // A hydration-requested ancestor id: present in `requested_ids` (dedup
    // set) AND cached in `self.events`. Without the pin, eviction would
    // remove it and the dedup check would block the re-fetch (broken
    // recovery — the reviewer's second finding).
    let hydrated_id = format!("{:0>64x}", 0xA00099u64);
    inject_tagged_note(&mut kernel, &hydrated_id, &make_pubkey(5_099), T0_SECS + 7, vec![]);
    kernel.thread_view.requested_ids.insert(hydrated_id.clone());

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
            "open-thread event {id} must survive eviction"
        );
    }

    // The view read path must still return the full set.
    let items = kernel.thread_items(&focused_id, &root_id);
    let item_ids: std::collections::HashSet<&str> =
        items.iter().map(|i| i.id.as_str()).collect();
    assert!(item_ids.contains(root_id.as_str()), "root must render");
    assert!(item_ids.contains(focused_id.as_str()), "focused must render");
    for id in &reply_ids {
        assert!(item_ids.contains(id.as_str()), "reply {id} must render");
    }
}

/// An OPEN author view's notes (a NON-followed author — not in
/// `timeline_authors`, not in `timeline`) must survive eviction, and
/// `author_items()` must still return all of them.
#[test]
fn open_author_view_events_survive_eviction() {
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

    // Open the author view through the REAL view path.
    kernel.open_author(
        author.clone(),
        std::collections::BTreeSet::from([1u32]),
        false,
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
            "open-author note {id} must survive eviction"
        );
    }

    let items = kernel.author_items(&author);
    assert_eq!(
        items.len(),
        note_ids.len(),
        "author_items must still return every note after eviction"
    );
}

/// The OPEN author view's profile (a non-followed, non-claimed author) must
/// survive profile eviction — `profile_for_pubkey()` has no store fallback.
#[test]
fn open_author_view_profile_survives_eviction() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS);

    // Flood profiles over the HWM; the FIRST injected profile (oldest
    // created_at → first eviction candidate) is the author we then open.
    let over = PROFILES_RAM_HWM + 74;
    let pubkeys = inject_profiles(&mut kernel, over, T0_SECS);
    let viewed_author = pubkeys[0].clone();

    kernel.open_author(
        viewed_author.clone(),
        std::collections::BTreeSet::from([1u32]),
        false,
    );

    kernel.evict_ram_caches();

    assert!(
        kernel.profiles.len() <= PROFILES_RAM_HWM,
        "cap must hold (len={})",
        kernel.profiles.len()
    );
    assert!(
        kernel.profiles.contains_key(&viewed_author),
        "open author view's profile must survive eviction"
    );
}

/// Thread PARTICIPANT profiles (authors of the open thread's replies) must
/// survive profile eviction — they feed `mention_profiles` /
/// `timeline_item()` for the open view.
#[test]
fn open_thread_participant_profiles_survive_eviction() {
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

    kernel.open_thread(
        root_id.clone(),
        std::collections::BTreeSet::from([1u32]),
        false,
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
