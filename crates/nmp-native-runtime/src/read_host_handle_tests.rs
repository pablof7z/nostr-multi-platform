//! #3080 — the read-session install/remove deferral is impossible-by-construction,
//! not merely timing-safe. These tests exercise the REAL [`NmpReadHost`] /
//! `NmpApp: ReadHost` path (not a fake test double), against the REAL actor
//! thread `crate::new_app()` spins up, so a regression back to synchronous
//! registry locking shows up as an actual cross-thread block, not a
//! same-thread self-deadlock that would hang the whole test binary.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use nmp_core::TypedProjectionData;
use nmp_ownership::DynamicProjectionKey;
use nmp_read_session::ReadHost;

/// Build a minimal opaque [`TypedProjectionData`] entry — payload bytes are
/// arbitrary, mirroring `nmp-core`'s own `typed_entry` test helper.
fn typed_entry(key: &str, payload: &[u8]) -> TypedProjectionData {
    TypedProjectionData {
        key: key.to_string(),
        schema_id: key.to_string(),
        schema_version: 1,
        file_identifier: "TEST".to_string(),
        payload: payload.to_vec(),
        ..Default::default()
    }
}

fn payload_for(app: &crate::NmpApp, key: &str) -> Option<Vec<u8>> {
    app.run_typed_snapshot_projections()
        .into_iter()
        .find(|d| d.key == key)
        .map(|d| d.payload)
}

/// Structural regression: `install_read_output` must never re-lock
/// `snapshot_projections` on the caller's thread. Proven by holding the
/// registry lock on the TEST thread and calling `install_read_output` from a
/// SEPARATE thread — if that call still tried to `.lock()` the same mutex
/// (the pre-#3080 synchronous body), it would block on real cross-thread
/// mutex contention forever. A bounded `recv_timeout` turns a regression into
/// a clean assertion failure instead of a hung test binary.
#[test]
fn install_read_output_never_relocks_the_registry_on_the_caller_thread() {
    let app = crate::new_app();
    app.start_runtime(50, 30);
    let read_host = app.read_host();
    const KEY: &str = "app.test.structural_install";

    // Hold the SAME lock a registered snapshot closure's execution context
    // would hold pre-#3079 (and that a re-entrant `install_read_output` used
    // to re-acquire, #3078).
    let guard = app.snapshot_projections.lock().unwrap();

    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        read_host.install_read_output(
            DynamicProjectionKey::app_owned(KEY).unwrap().into(),
            Box::new(|| None),
        );
        let _ = tx.send(());
    });

    assert!(
        rx.recv_timeout(Duration::from_secs(5)).is_ok(),
        "install_read_output blocked trying to re-lock snapshot_projections while \
         another thread held it — #3080 regression (the exact door #3080 closes)"
    );
    drop(guard);

    assert!(
        app.wait_barrier_for_test(Duration::from_secs(5)),
        "actor did not drain the deferred install"
    );
    assert!(
        app.registered_typed_projection_keys()
            .contains(&KEY.to_string()),
        "the deferred install must still apply once the actor drains its inbox"
    );
}

/// Functional (29er#60 miniature): a snapshot-projection closure that opens a
/// NESTED read-session output — captured `NmpReadHost`, exactly the pattern
/// #3078 traced group-discovery's per-row fan-out to — must go live after a
/// drain, without the outer tick itself blocking.
#[test]
fn read_output_opened_from_a_snapshot_closure_goes_live_after_drain() {
    let app = crate::new_app();
    app.start_runtime(50, 30);
    let read_host = app.read_host();
    const OUTER_KEY: &str = "app.test.mini29er.outer";
    const INNER_KEY: &str = "app.test.mini29er.inner";

    let opened = Arc::new(AtomicBool::new(false));
    let opened_in_closure = Arc::clone(&opened);
    app.register_typed_snapshot_projection_with_time(
        DynamicProjectionKey::app_owned(OUTER_KEY).unwrap(),
        move |_now| {
            // Open-once, mirroring a group-directory row that installs its
            // per-row read on first sight. Legally captures `NmpReadHost` —
            // the same door `Arc<NmpApp>` gives a closure.
            if !opened_in_closure.swap(true, Ordering::SeqCst) {
                read_host.install_read_output(
                    DynamicProjectionKey::app_owned(INNER_KEY).unwrap().into(),
                    Box::new(|| Some(typed_entry(INNER_KEY, b"live"))),
                );
            }
            None
        },
    );

    // Drive one tick: runs the outer closure (via the out-of-band
    // introspection accessor), which opens the inner output.
    let _ = app.run_typed_snapshot_projections();
    assert!(
        app.wait_barrier_for_test(Duration::from_secs(5)),
        "actor did not drain the deferred install opened from the closure"
    );

    // Drive a second tick to observe the now-installed inner output.
    assert_eq!(
        payload_for(&app, INNER_KEY),
        Some(b"live".to_vec()),
        "a read-session output opened from a snapshot closure must go live after drain"
    );
}

/// Reopen ordering: close-by-key then reopen the SAME key within one turn (no
/// barrier between) must apply in FIFO order — the reopen's install wins,
/// exactly mirroring the "install enqueued right after remove for the same
/// key" shape every production open/close-then-reopen produces (e.g. NIP-50's
/// `open_search_read`, which closes-by-key before opening the replacement).
#[test]
fn reopen_same_key_within_one_turn_applies_in_fifo_order() {
    let app = crate::new_app();
    app.start_runtime(50, 30);
    let read_host = app.read_host();
    const KEY: &str = "app.test.reopen_fifo";

    read_host.install_read_output(
        DynamicProjectionKey::app_owned(KEY).unwrap().into(),
        Box::new(|| Some(typed_entry(KEY, b"v1"))),
    );
    assert!(app.wait_barrier_for_test(Duration::from_secs(5)));
    assert_eq!(payload_for(&app, KEY), Some(b"v1".to_vec()));

    // Close-by-key, then reopen the SAME key — both enqueued back to back on
    // the ONE FIFO actor inbox, with NO barrier between them.
    (read_host.teardown_remove_output(KEY.to_string()))();
    read_host.install_read_output(
        DynamicProjectionKey::app_owned(KEY).unwrap().into(),
        Box::new(|| Some(typed_entry(KEY, b"v2"))),
    );

    assert!(app.wait_barrier_for_test(Duration::from_secs(5)));
    assert_eq!(
        payload_for(&app, KEY),
        Some(b"v2".to_vec()),
        "the install enqueued after the remove for the same key must win (FIFO)"
    );
}

/// Fail-on-master regression for the SECOND #3078-class deadlock door:
/// `NmpApp::run_typed_snapshot_projections` (the NIP-50 search-poll host's
/// accessor, `search.rs`'s `search_snapshot_payload`) used to run registered
/// closures WHILE holding the registry lock. Mirrors nmp-core's own
/// `typed_closure_runs_with_registry_lock_released` (#3079) — a deterministic
/// `try_lock` probe, not a real read-session open, because after #3080 there
/// is no synchronous re-lock door left through the `ReadHost` seam to probe
/// with; this test targets `run_typed_snapshot_projections` directly.
#[test]
fn run_typed_snapshot_projections_runs_closures_with_registry_lock_released() {
    let app = crate::new_app();
    let slot = Arc::clone(&app.snapshot_projections);

    let observed_lock_free = Arc::new(AtomicBool::new(false));
    let probe = Arc::clone(&observed_lock_free);
    let reentrant = Arc::clone(&slot);
    app.register_typed_snapshot_projection_with_time(
        DynamicProjectionKey::app_owned("app.test.search_poll_probe").unwrap(),
        move |_now| {
            if reentrant.try_lock().is_ok() {
                probe.store(true, Ordering::SeqCst);
            }
            None
        },
    );

    let _ = app.run_typed_snapshot_projections();

    assert!(
        observed_lock_free.load(Ordering::SeqCst),
        "run_typed_snapshot_projections (the NIP-50 search-poll accessor) ran a \
         registered closure WHILE holding the registry lock — a read-session \
         open/close from that closure would deadlock a host thread polling \
         search results against itself (#3080's second door)"
    );
}

/// Same regression, exercised end-to-end through the REAL `ReadHost` seam
/// instead of a raw `try_lock` probe: a closure that opens a nested read
/// output while `run_typed_snapshot_projections` is mid-run (simulating a
/// host thread polling NIP-50 search results) must complete rather than hang.
/// Spawn + bounded `recv_timeout` so a regression fails the assertion instead
/// of hanging the test binary.
#[test]
fn run_typed_snapshot_projections_completes_when_a_closure_reenters_install() {
    let app = Arc::new(crate::new_app());
    let read_host = app.read_host();
    const OUTER_KEY: &str = "app.test.search_poll_outer";
    const INNER_KEY: &str = "app.test.search_poll_inner";

    app.register_typed_snapshot_projection_with_time(
        DynamicProjectionKey::app_owned(OUTER_KEY).unwrap(),
        move |_now| {
            read_host.install_read_output(
                DynamicProjectionKey::app_owned(INNER_KEY).unwrap().into(),
                Box::new(|| None),
            );
            None
        },
    );

    let app2 = Arc::clone(&app);
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        // Stands in for `SearchHost::search_snapshot_payload`'s call into
        // `run_typed_snapshot_projections` from a host-owned thread.
        let _ = app2.run_typed_snapshot_projections();
        let _ = tx.send(());
    });

    assert!(
        rx.recv_timeout(Duration::from_secs(5)).is_ok(),
        "run_typed_snapshot_projections deadlocked against its own registry lock \
         when a registered closure re-entered install_read_output — #3080 regression"
    );
}
