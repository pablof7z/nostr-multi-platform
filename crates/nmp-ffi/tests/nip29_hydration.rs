//! #2088 acceptance — NIP-29 per-open views HYDRATE already-cached events.
//!
//! The bug: the four NIP-29 per-open views registered their projection as a
//! bare, already-active `KernelEventObserver`, which only sees the global
//! fan-out of LIVE ingest. A view opened AFTER its events were accepted +
//! cached hydrated live-only and silently dropped the cached tail.
//!
//! The fix: `NmpApp::open_group_timeline` / `open_group_discovery` /
//! `open_joined_groups` register the projection MUTED and route ingest through
//! `open_observed_interest_pinned`, whose ADR-0062 read-cache replay delivers
//! the matching cached events to the muted observer (matched by the `#h` /
//! kind shapes built from the same wire filter) BEFORE activating it.
//!
//! These tests inject events BEFORE opening the view, then assert the typed
//! snapshot the view emits already contains them — the exact "view opened after
//! events cached" sequence the bug regressed.
//!
//! The in-memory read-cache (`Kernel::events`, a bounded LRU populated for every
//! admitted event) is what the replay scans. The DURABLE-store tail (events
//! evicted from that cache) hydrates once the general single-letter (`#h`)
//! `StoreQuery` index lands; that substrate is out of scope here and these
//! tests stay within the in-memory window (a handful of events, never evicted).

mod common;

use common::{boot, inject, raw_event, teardown, wait_for_typed, HOST, SERIAL};

use nmp_store::VerifiedEvent;
use nmp_nip29::group_id::GroupId;
use nmp_nip29::{decode_discovered_groups_snapshot, decode_group_timeline_snapshot};

/// THE #2088 FIX (group chat): kind:9/11 events for a group, cached BEFORE the
/// chat view opens, surface in the view's first snapshot — the catch-up the bare
/// live-only observer dropped.
#[test]
fn group_timeline_hydrates_events_cached_before_open() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let app = boot();

    // Cache two group-chat events BEFORE the view is opened.
    let first = VerifiedEvent::from_raw_unchecked(raw_event(
        &"a".repeat(64),
        &"b".repeat(64),
        9,
        1_700_000_000,
        vec![vec!["h".into(), "preopen-room".into()]],
        "cached before open",
    ));
    let second = VerifiedEvent::from_raw_unchecked(raw_event(
        &"c".repeat(64),
        &"d".repeat(64),
        11,
        1_700_000_001,
        vec![vec!["h".into(), "preopen-room".into()]],
        "also cached before open",
    ));
    inject(app, vec![first, second]);

    // Open the view AFTER the events are already cached — the #2088 sequence.
    // SAFETY: `app` is a valid pointer from `nmp_app_new`, live for this block.
    unsafe { (*app).open_group_timeline(GroupId::new(HOST, "preopen-room")) };

    let entry = wait_for_typed("nmp.nip29.group_timeline", |t| {
        decode_group_timeline_snapshot(&t.payload)
            .map(|s| {
                s.events.iter().any(|m| m.content == "cached before open")
                    && s.events.iter().any(|m| m.content == "also cached before open")
            })
            .unwrap_or(false)
    })
    .expect("group chat must hydrate BOTH events cached before open (#2088) within 3 s");

    let snapshot = decode_group_timeline_snapshot(&entry.payload).expect("NGTL decode");
    assert_eq!(
        snapshot.events.len(),
        2,
        "exactly the two pre-cached group messages hydrate, got {:?}",
        snapshot.events
    );

    teardown(app);
}

/// NEGATIVE: an event for a DIFFERENT group (`#h` value mismatch), cached before
/// open, must NOT hydrate into the opened group's view — the `#h` replay shape
/// gates by the group's `local_id`.
#[test]
fn group_timeline_hydration_excludes_other_group() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let app = boot();

    // One matching event + one for a foreign group, both cached before open.
    let mine = VerifiedEvent::from_raw_unchecked(raw_event(
        &"1".repeat(64),
        &"2".repeat(64),
        9,
        1_700_000_010,
        vec![vec!["h".into(), "target".into()]],
        "belongs here",
    ));
    let foreign = VerifiedEvent::from_raw_unchecked(raw_event(
        &"3".repeat(64),
        &"4".repeat(64),
        9,
        1_700_000_011,
        vec![vec!["h".into(), "other-room".into()]],
        "foreign group",
    ));
    inject(app, vec![mine, foreign]);

    unsafe { (*app).open_group_timeline(GroupId::new(HOST, "target")) };

    let entry = wait_for_typed("nmp.nip29.group_timeline", |t| {
        decode_group_timeline_snapshot(&t.payload)
            .map(|s| s.events.iter().any(|m| m.content == "belongs here"))
            .unwrap_or(false)
    })
    .expect("the matching pre-cached event must hydrate within 3 s");

    let snapshot = decode_group_timeline_snapshot(&entry.payload).expect("NGTL decode");
    assert!(
        !snapshot
            .events
            .iter()
            .any(|m| m.content == "foreign group"),
        "an event for a different group (#h mismatch) must NOT hydrate into this view"
    );
    assert_eq!(snapshot.events.len(), 1, "only the matching group event");

    teardown(app);
}

/// THE #2088 FIX (discovery): kind:39000/39002 metadata for a relay, cached
/// BEFORE the discovery view opens, rolls into the view's first snapshot.
#[test]
fn discovered_groups_hydrate_metadata_cached_before_open() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let app = boot();

    let meta = VerifiedEvent::from_raw_unchecked(raw_event(
        &"5".repeat(64),
        &"f".repeat(64),
        39000,
        100,
        vec![
            vec!["d".into(), "preopen-group".into()],
            vec!["name".into(), "Pre-Open Group".into()],
        ],
        "",
    ));
    let members = VerifiedEvent::from_raw_unchecked(raw_event(
        &"6".repeat(64),
        &"f".repeat(64),
        39002,
        101,
        vec![
            vec!["d".into(), "preopen-group".into()],
            vec!["p".into(), "x".repeat(64)],
            vec!["p".into(), "y".repeat(64)],
        ],
        "",
    ));
    inject(app, vec![meta, members]);

    // Open discovery AFTER the catalog is already cached — the #2088 sequence.
    let _handle = unsafe { (*app).open_group_discovery(HOST.to_string()) };

    let entry = wait_for_typed("nmp.nip29.discovered_groups", |t| {
        decode_discovered_groups_snapshot(&t.payload)
            .map(|s| {
                s.groups
                    .iter()
                    .any(|g| g.group_id == "preopen-group" && g.member_count == 2)
            })
            .unwrap_or(false)
    })
    .expect("discovery must hydrate the relay catalog cached before open (#2088) within 3 s");

    let snapshot = decode_discovered_groups_snapshot(&entry.payload).expect("NDGS decode");
    let group = snapshot
        .groups
        .iter()
        .find(|g| g.group_id == "preopen-group")
        .expect("the pre-cached group hydrates");
    assert_eq!(group.name.as_deref(), Some("Pre-Open Group"));
    assert_eq!(group.member_count, 2);

    teardown(app);
}
