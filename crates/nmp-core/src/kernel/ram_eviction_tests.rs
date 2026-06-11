//! #1088 — RAM-tier eviction TDD tests.
//!
//! Invariants pinned here:
//!
//! 1. After `evict_ram_caches` fires, `events.len() ≤ EVENTS_RAM_HWM`.
//! 2. After `evict_ram_caches` fires, `profiles.len() ≤ PROFILES_RAM_HWM`.
//! 3. After `evict_ram_caches` fires, `seed_contacts.len() ≤ SEED_CONTACTS_RAM_HWM`.
//! 4. **Live-ref invariant** — entries that are claimed / in the timeline /
//!    followed / owned by the active account are NEVER evicted.
//! 5. `metric_stored_events` is decremented for every evicted `events` entry.
//! 6. Maps that are already under the HWM are not touched.
//! 7. `run_gc_step` drives `evict_ram_caches` (integration path).
//! 8. **Open-view invariant** (Opus review on PR #1096) — an open thread
//!    view's root + replies + hydration-requested ids, an open author view's
//!    notes (non-followed author), and the open views' author profiles all
//!    survive eviction, and `thread_items()`/`author_items()` still return
//!    the full set afterwards.  `open_thread`/`open_author` write NOTHING to
//!    `event_claims`, so these pins must derive from the live view state.
//!
//! Test strategy: insert events/profiles/seed_contacts through the real
//! ingest path (using `ingest_pre_verified_event` / `inject_replaceable_event`
//! to stay signature-free in unit-test builds), drive the clock with
//! `FixedClock`, call `evict_ram_caches`, and assert.

use super::clock::FixedClock;
use super::ram_eviction::{EVENTS_RAM_HWM, PROFILES_RAM_HWM, SEED_CONTACTS_RAM_HWM};
use super::*;
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::store::{RawEvent, VerifiedEvent};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

pub(super) const RELAY_A: &str = "wss://a.example/";
pub(super) const T0_SECS: u64 = 1_700_000_000;

pub(super) fn pin_clock(kernel: &mut Kernel, secs: u64) {
    let fixed = SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
    kernel.set_clock(Arc::new(FixedClock(fixed)));
}

pub(super) fn make_event_id(n: usize) -> String {
    format!("{:0>64}", n)
}

pub(super) fn make_pubkey(n: usize) -> String {
    format!("{:0>64x}", n)
}

/// Insert `count` unique kind:1 events via `ingest_pre_verified_event` so
/// they land in `self.events`.  Returns the list of inserted event ids.
pub(super) fn inject_events(kernel: &mut Kernel, count: usize, base_created_at: u64) -> Vec<String> {
    let mut ids = Vec::with_capacity(count);
    for i in 0..count {
        let id = make_event_id(i + 1);
        let pubkey = make_pubkey((i % 50) + 1); // 50 distinct authors
        let raw = RawEvent {
            id: id.clone(),
            pubkey,
            created_at: base_created_at + i as u64,
            kind: 1,
            tags: vec![],
            content: format!("note {i}"),
            sig: "a".repeat(128),
        };
        let verified = VerifiedEvent::from_raw_unchecked(raw);
        // Use an empty sub_id to avoid auto-appending to timeline — this test
        // controls the timeline manually to test the pin invariant.
        kernel.ingest_pre_verified_event(RelayRole::Content, "", verified);
        ids.push(id);
    }
    ids
}

/// Insert `count` unique kind:0 profile events via `inject_replaceable_event`.
pub(super) fn inject_profiles(kernel: &mut Kernel, count: usize, base_created_at: u64) -> Vec<String> {
    let mut pubkeys = Vec::with_capacity(count);
    for i in 0..count {
        let pubkey = make_pubkey(1_000 + i + 1); // distinct from event authors
        // Event id must also be valid 64-char hex — use offset 0x10000 to
        // avoid colliding with `make_event_id` (which uses decimal-padded).
        let id = format!("{:0>64x}", 0x10000usize + i + 1);
        kernel.inject_replaceable_event(
            &id,
            &pubkey,
            base_created_at + i as u64,
            0,
            vec![],
            RELAY_A,
            (base_created_at + i as u64) * 1_000,
        );
        pubkeys.push(pubkey);
    }
    pubkeys
}

// ─── 1. events cap ─────────────────────────────────────────────────────────

/// Inserting more than `EVENTS_RAM_HWM` events and calling `evict_ram_caches`
/// must bring the map down to or below the HWM.
#[test]
fn events_eviction_caps_at_hwm() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS);

    let over = EVENTS_RAM_HWM + 64 + 10;
    inject_events(&mut kernel, over, T0_SECS);

    assert!(
        kernel.events.len() > EVENTS_RAM_HWM,
        "precondition: events must exceed HWM before eviction (len={})",
        kernel.events.len()
    );

    let report = kernel.evict_ram_caches();
    assert!(
        kernel.events.len() <= EVENTS_RAM_HWM,
        "after eviction events.len() must be ≤ HWM={EVENTS_RAM_HWM}, got {}",
        kernel.events.len()
    );
    assert!(
        report.events_evicted > 0,
        "must report at least one eviction"
    );
}

/// No eviction when the map is already within bounds.
#[test]
fn events_no_eviction_under_hwm() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS);
    inject_events(&mut kernel, 10, T0_SECS);

    let before = kernel.events.len();
    let report = kernel.evict_ram_caches();
    assert_eq!(
        kernel.events.len(),
        before,
        "under-HWM map must not shrink"
    );
    assert_eq!(report.events_evicted, 0);
}

// ─── 2. profiles cap ───────────────────────────────────────────────────────

/// Inserting more than `PROFILES_RAM_HWM` profiles and calling
/// `evict_ram_caches` must bring `profiles.len()` down to or below the HWM.
#[test]
fn profiles_eviction_caps_at_hwm() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS);

    let over = PROFILES_RAM_HWM + 64 + 10;
    inject_profiles(&mut kernel, over, T0_SECS);

    assert!(
        kernel.profiles.len() > PROFILES_RAM_HWM,
        "precondition: profiles must exceed HWM (len={})",
        kernel.profiles.len()
    );

    let report = kernel.evict_ram_caches();
    assert!(
        kernel.profiles.len() <= PROFILES_RAM_HWM,
        "after eviction profiles.len() must be ≤ HWM={PROFILES_RAM_HWM}, got {}",
        kernel.profiles.len()
    );
    assert!(report.profiles_evicted > 0);
}

// ─── 3. seed_contacts cap ──────────────────────────────────────────────────

/// Inserting more than `SEED_CONTACTS_RAM_HWM` contacts and calling
/// `evict_ram_caches` must bring `seed_contacts.len()` down.
#[test]
fn seed_contacts_eviction_caps_at_hwm() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS);

    let over = SEED_CONTACTS_RAM_HWM + 64 + 5;
    for i in 0..over {
        let pk = make_pubkey(2_000 + i + 1);
        kernel.prepopulate_seed_contacts(pk, vec![]);
    }

    assert!(
        kernel.seed_contacts.len() > SEED_CONTACTS_RAM_HWM,
        "precondition: seed_contacts must exceed HWM (len={})",
        kernel.seed_contacts.len()
    );

    let report = kernel.evict_ram_caches();
    assert!(
        kernel.seed_contacts.len() <= SEED_CONTACTS_RAM_HWM,
        "after eviction seed_contacts.len() must be ≤ HWM={SEED_CONTACTS_RAM_HWM}, got {}",
        kernel.seed_contacts.len()
    );
    assert!(report.seed_contacts_evicted > 0);
}

// ─── 4. live-ref invariant ─────────────────────────────────────────────────

/// Events in `timeline` must NEVER be evicted.
#[test]
fn timeline_events_are_never_evicted() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS);

    // Insert enough events to exceed the HWM.
    let over = EVENTS_RAM_HWM + 64 + 50;
    let ids = inject_events(&mut kernel, over, T0_SECS);

    // Take the first 20 ids and manually add them to the timeline — these
    // are the "visible" entries.
    let pinned_ids: Vec<String> = ids[..20].to_vec();
    for id in &pinned_ids {
        kernel.timeline.push_front(id.clone());
    }

    kernel.evict_ram_caches();

    for id in &pinned_ids {
        assert!(
            kernel.events.contains_key(id),
            "timeline event {id} must survive eviction"
        );
    }
}

/// Events held in `event_claims` must NEVER be evicted.
#[test]
fn claimed_events_are_never_evicted() {
    use std::collections::BTreeSet;

    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS);

    let over = EVENTS_RAM_HWM + 64 + 50;
    let ids = inject_events(&mut kernel, over, T0_SECS);

    // Simulate a UI component claiming the first 10 events.
    let claimed_ids: Vec<String> = ids[..10].to_vec();
    for id in &claimed_ids {
        kernel
            .event_claims
            .entry(id.clone())
            .or_insert_with(BTreeSet::new)
            .insert("consumer-A".to_string());
    }

    kernel.evict_ram_caches();

    for id in &claimed_ids {
        assert!(
            kernel.events.contains_key(id),
            "claimed event {id} must survive eviction"
        );
    }
}

/// Profiles in `profile_claims` must NEVER be evicted.
#[test]
fn claimed_profiles_are_never_evicted() {
    use std::collections::BTreeSet;

    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS);

    let over = PROFILES_RAM_HWM + 64 + 50;
    let pubkeys = inject_profiles(&mut kernel, over, T0_SECS);

    let claimed: Vec<String> = pubkeys[..10].to_vec();
    for pk in &claimed {
        kernel
            .profile_claims
            .entry(pk.clone())
            .or_insert_with(BTreeSet::new)
            .insert("consumer-A".to_string());
    }

    kernel.evict_ram_caches();

    for pk in &claimed {
        assert!(
            kernel.profiles.contains_key(pk),
            "claimed profile {pk} must survive eviction"
        );
    }
}

/// Profiles for followed authors (`timeline_authors`) must NEVER be evicted.
#[test]
fn followed_profiles_are_never_evicted() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS);

    let over = PROFILES_RAM_HWM + 64 + 50;
    let pubkeys = inject_profiles(&mut kernel, over, T0_SECS);

    // Mark the first 20 as followed.
    let followed: Vec<String> = pubkeys[..20].to_vec();
    for pk in &followed {
        kernel.timeline_authors.insert(pk.clone());
    }

    kernel.evict_ram_caches();

    for pk in &followed {
        assert!(
            kernel.profiles.contains_key(pk),
            "followed profile {pk} must survive eviction"
        );
    }
}

/// The active account's profile must NEVER be evicted.
#[test]
fn active_account_profile_is_never_evicted() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS);

    let over = PROFILES_RAM_HWM + 64 + 50;
    let pubkeys = inject_profiles(&mut kernel, over, T0_SECS);

    // Set the first profile as the active account.
    let active_pk = pubkeys[0].clone();
    kernel.active_account = Some(active_pk.clone());

    kernel.evict_ram_caches();

    assert!(
        kernel.profiles.contains_key(&active_pk),
        "active account profile must survive eviction"
    );
}

/// Active account's seed_contacts entry must NEVER be evicted.
#[test]
fn active_account_seed_contacts_are_never_evicted() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS);

    let active_pk = make_pubkey(9999);
    kernel.active_account = Some(active_pk.clone());
    kernel.prepopulate_seed_contacts(active_pk.clone(), vec!["follow1".to_string()]);

    let over = SEED_CONTACTS_RAM_HWM + 64 + 5;
    for i in 0..over {
        let pk = make_pubkey(3_000 + i + 1);
        kernel.prepopulate_seed_contacts(pk, vec![]);
    }

    assert!(
        kernel.seed_contacts.len() > SEED_CONTACTS_RAM_HWM,
        "precondition: must exceed HWM"
    );

    kernel.evict_ram_caches();

    assert!(
        kernel.seed_contacts.contains_key(&active_pk),
        "active account seed_contacts must survive eviction"
    );
}

// ─── 5. metric_stored_events decremented ───────────────────────────────────

/// `metric_stored_events` must be decremented for every event evicted.
#[test]
fn metric_stored_events_is_decremented_on_eviction() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS);

    let over = EVENTS_RAM_HWM + 64 + 10;
    inject_events(&mut kernel, over, T0_SECS);

    let before_metric = kernel.metric_stored_events;
    let before_len = kernel.events.len() as u64;
    assert_eq!(
        before_metric, before_len,
        "precondition: metric must match events.len() before eviction"
    );

    let report = kernel.evict_ram_caches();
    assert!(report.events_evicted > 0, "at least one eviction expected");
    assert_eq!(
        kernel.metric_stored_events,
        before_metric - report.events_evicted as u64,
        "metric_stored_events must track each evicted entry"
    );
    assert_eq!(
        kernel.metric_stored_events,
        kernel.events.len() as u64,
        "metric_stored_events must equal events.len() after eviction"
    );
}

// ─── 7. run_gc_step drives evict_ram_caches ────────────────────────────────

/// `run_gc_step` must call `evict_ram_caches`.  Verified by inserting >HWM
/// events, calling `run_gc_step`, and asserting the map shrank.
#[test]
fn run_gc_step_drives_ram_eviction() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS);

    let over = EVENTS_RAM_HWM + 64 + 10;
    inject_events(&mut kernel, over, T0_SECS);

    assert!(
        kernel.events.len() > EVENTS_RAM_HWM,
        "precondition: must exceed HWM"
    );

    // run_gc_step drives the whole GC pass including RAM eviction.
    kernel.run_gc_step();

    assert!(
        kernel.events.len() <= EVENTS_RAM_HWM,
        "events must be capped after run_gc_step (len={})",
        kernel.events.len()
    );
}

