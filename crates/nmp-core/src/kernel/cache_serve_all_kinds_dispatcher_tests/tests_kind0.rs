use super::*;

// ─── ADR-0057 unified post-store projection on the cache-serve path ───────────
//
// These prove the unification codex required: cache-serve replay runs the SAME
// `Kernel::project_accepted_event` the live chokepoint runs, so the capability-
// cache transition sweep AND the D9 clamp fire on the cache-serve path too. The
// non-vacuity note on each: removing the shared-helper call from
// `feed_served_event` (or its transition sweep / clamp) fails the test.

/// (a) #1 FIX — a stored kind:0 served from the store on a cold restart bumps
/// `profiles_ver` (via the shared `project_accepted_event` transition sweep) AND
/// populates the capability profile cache, so incremental profile projections
/// re-emit instead of staying `Unchanged` (stale UI). Non-vacuous: deleting the
/// `project_accepted_event` call from `feed_served_event` leaves the cache empty
/// and `profiles_ver` unbumped — both assertions fail.
#[test]
fn cache_served_kind0_bumps_profiles_ver_and_populates_cache() {
    let base_ts: u64 = 1_700_000_300;
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(hex_pk("aa"));
    kernel.timeline_authors.insert(author.clone());
    let profile_lookup = Arc::new(TestProfileLookup::new());
    kernel.set_profile_lookup(Arc::clone(&profile_lookup) as Arc<dyn ProfileLookup>);
    if let Ok(mut d) = kernel.ingest_dispatcher_slot().write() {
        d.register_kind(0, ProfileViewWriterParser::new(Arc::clone(&profile_lookup), "Nova"));
    }

    // Phase 1: live-ingest a kind:0 into the store (also populates the cache).
    live_ingest(
        &mut kernel,
        "follow-feed-default",
        &signed_kind0(&keys, "Nova", base_ts),
    );
    assert_eq!(
        kernel.profile_lookup().profile(&author).map(|p| p.display),
        Some("Nova".to_string()),
        "precondition: live kind:0 populated the profile cache",
    );

    // Phase 2: cold restart — clear in-memory caches AND the capability profile
    // cache, so the cache-serve replay is the ONLY thing that can repopulate it.
    simulate_cold_restart(&mut kernel);
    kernel
        .profile_lookup()
        .evict_to(&std::collections::HashSet::new(), 0);
    assert!(
        kernel.profile_lookup().profile(&author).is_none(),
        "profile cache cleared before cache-serve replay",
    );
    let ver_before = profiles_ver(&kernel);

    // Phase 3: replay via cache-serve with ZERO relays.
    open_kind0_interest(&mut kernel, 30, &author);
    drain_cache_serves(&mut kernel, 10);

    // Phase 4: the shared helper re-wrote the cache + bumped the rev.
    assert_eq!(
        kernel.profile_lookup().profile(&author).map(|p| p.display),
        Some("Nova".to_string()),
        "cache-served kind:0 must repopulate the capability profile cache via \
         the shared project_accepted_event → registered profile-view writer",
    );
    assert!(
        profiles_ver(&kernel) > ver_before,
        "cache-served kind:0 must bump profiles_ver so incremental profile \
         projections re-emit after cold restart ({ver_before} -> {})",
        profiles_ver(&kernel),
    );
}

/// (b) PORTED from PR1b — a FUTURE-dated event served from the store on cold
/// restart is clamped to `now` in the observer fan-out (D9), via the SAME shared
/// helper. The store + read-cache retain the raw wire timestamp. Non-vacuous:
/// removing the clamp in `project_accepted_event` makes the observer see
/// NOW + 9_999.
#[test]
fn cache_served_future_dated_event_is_clamped_in_fan_out() {
    use crate::actor::{new_event_observer_slot, register_rust_observer, ObservedProjectionSink};
    use crate::kernel::clock::FixedClock;
    use crate::substrate::KernelEvent;
    use std::collections::HashMap;
    use std::time::{Duration, SystemTime};

    struct CapturingObserver {
        seen: Mutex<HashMap<String, u64>>,
    }
    impl ObservedProjectionSink for CapturingObserver {
        fn on_kernel_event(&self, event: &KernelEvent) {
            self.seen
                .lock()
                .unwrap()
                .insert(event.id.clone(), event.created_at);
        }
    }

    const NOW_SECS: u64 = 1_700_000_000;
    let fixed = SystemTime::UNIX_EPOCH + Duration::from_secs(NOW_SECS);

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_clock(Arc::new(FixedClock(fixed)));

    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    kernel.active_account = Some(hex_pk("aa"));
    kernel.timeline_authors.insert(author.clone());

    let future = signed_note(&keys, "from the future", NOW_SECS + 9_999);
    let past = signed_note(&keys, "from the past", NOW_SECS - 500_000);
    let future_id = future.id.clone();
    let past_id = past.id.clone();
    kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://seed.relay/",
        "follow-feed-default",
        future,
    );
    kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://seed.relay/",
        "follow-feed-default",
        past,
    );
    assert_eq!(
        kernel.events.len(),
        2,
        "both seeded events in cache pre-restart"
    );

    simulate_cold_restart(&mut kernel);
    assert!(
        kernel.events.is_empty(),
        "events cache empty after cold restart"
    );

    // Observer registered AFTER restart → captures ONLY the cache-serve fan-out.
    let slot = new_event_observer_slot();
    let observer = Arc::new(CapturingObserver {
        seen: Mutex::new(HashMap::new()),
    });
    register_rust_observer(&slot, observer.clone());
    kernel.set_event_observers_handle(slot);

    open_author_interest(&mut kernel, 31, &author);
    drain_cache_serves(&mut kernel, 4);

    let seen = observer.seen.lock().unwrap();
    assert_eq!(
        seen.get(&future_id).copied(),
        Some(NOW_SECS),
        "future-dated created_at served from the store must be clamped to now in \
         the observer fan-out (D9, via the shared project_accepted_event)",
    );
    assert_eq!(
        seen.get(&past_id).copied(),
        Some(NOW_SECS - 500_000),
        "past-dated created_at passes through unchanged — clamp is min(wire, now)",
    );
    drop(seen);

    assert_eq!(
        kernel.events[future_id.as_str()].created_at,
        NOW_SECS + 9_999,
        "the served StoredEvent retains the unclamped wire created_at; only the \
         observer payload is clamped",
    );
}
