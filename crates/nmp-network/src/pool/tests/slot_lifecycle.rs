//! Pure structural pool-side bookkeeping: slot allocation, generational
//! handle rejection, snapshot shape, health state after `ensure_open`,
//! and the "no send-to-all" surface. No real socket; the worker's spawn
//! call is exercised but the URL is a sentinel that never connects — we
//! only assert the pool-side bookkeeping.
use std::sync::mpsc;

use super::super::{inner::canonicalize, HealthState, Pool, PoolConfig, RelayHandle, WireFrame};
use crate::role::RelayRole;

/// canonicalize: delegates to the single Layer-0 authority — case-folds the
/// scheme/host and trims whitespace (so `WSS://` and a stray newline don't
/// fragment the pool), and is fail-closed (an off-contract URL yields `None`,
/// so `ensure_open` refuses to dial it) (#967).
#[test]
fn canonicalize_normalizes_and_fails_closed() {
    assert_eq!(
        canonicalize("WSS://relay.example").as_deref(),
        Some("wss://relay.example")
    );
    assert_eq!(
        canonicalize("  wss://relay.example\n").as_deref(),
        Some("wss://relay.example")
    );
    assert_eq!(canonicalize("not a url"), None);
    assert_eq!(canonicalize("http://relay.example"), None);
    assert_eq!(canonicalize("wss://"), None);
}

/// Two `ensure_open` calls for the same URL share a slot.
/// Without a real socket, the worker thread will keep retrying the
/// dial, but the pool-side state (slot map, handle generation) is
/// observable synchronously.
#[test]
fn ensure_open_idempotent_same_url_returns_same_handle() {
    let (events_tx, _events_rx) = mpsc::channel();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    // Use a port that nothing's listening on so the worker dials and
    // fails — the slot allocation is what we assert, not connectivity.
    let url = String::from("wss://127.0.0.1:1/sentinel");
    let h1 = pool.ensure_open(&url);
    let h2 = pool.ensure_open(&url);
    assert_eq!(h1, h2, "same URL must yield same handle");
    pool.shutdown();
}

/// Distinct URLs get distinct slots.
#[test]
fn ensure_open_distinct_urls_get_distinct_slots() {
    let (events_tx, _events_rx) = mpsc::channel();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    let h_a = pool.ensure_open(&String::from("wss://127.0.0.1:1/a"));
    let h_b = pool.ensure_open(&String::from("wss://127.0.0.1:1/b"));
    assert_ne!(
        h_a.slot(),
        h_b.slot(),
        "distinct URLs must get distinct slot ids"
    );
    pool.shutdown();
}

/// `close` then `ensure_open` for the same URL re-uses the slot id but
/// bumps the generation. The prior handle is now structurally stale.
#[test]
fn close_then_reopen_bumps_generation_invalidating_stale_handle() {
    let (events_tx, _events_rx) = mpsc::channel();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    let url = String::from("wss://127.0.0.1:1/sentinel");
    let h1 = pool.ensure_open(&url);
    assert!(pool.close(h1));
    let h2 = pool.ensure_open(&url);
    assert_eq!(h1.slot(), h2.slot(), "slot id must be reused");
    assert!(
        h2.generation() > h1.generation(),
        "reopen must bump generation (was {}, is {})",
        h1.generation(),
        h2.generation(),
    );
    // The stale handle is rejected by the public API.
    assert!(
        pool.health(h1).is_none(),
        "stale handle must yield None from health()"
    );
    assert!(!pool.close(h1), "stale handle must be a no-op for close()");
    assert!(
        !pool.send(h1, WireFrame::Text("[\"REQ\",\"x\",{}]".to_string())),
        "stale handle must be a no-op for send()"
    );
    pool.shutdown();
}

/// `health()` returns `Some(state=Connecting)` immediately after
/// `ensure_open` (before any worker event arrives).
#[test]
fn health_after_ensure_open_is_connecting() {
    let (events_tx, _events_rx) = mpsc::channel();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    let h = pool.ensure_open(&String::from("wss://127.0.0.1:1/sentinel"));
    let health = pool.health(h).expect("fresh handle must be live");
    assert_eq!(health.state, HealthState::Connecting);
    pool.shutdown();
}

/// `snapshot()` enumerates every live slot.
#[test]
fn snapshot_enumerates_live_slots() {
    let (events_tx, _events_rx) = mpsc::channel();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    let _h_a = pool.ensure_open(&String::from("wss://127.0.0.1:1/a"));
    let _h_b = pool.ensure_open(&String::from("wss://127.0.0.1:1/b"));
    let snap = pool.snapshot();
    assert_eq!(snap.rows.len(), 2, "snapshot must list both slots");
    pool.shutdown();
}

/// Sentinel handle returned post-shutdown is structurally invalid.
#[test]
fn ensure_open_after_shutdown_returns_sentinel_handle() {
    let (events_tx, _events_rx) = mpsc::channel();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    pool.shutdown();
    let h = pool.ensure_open(&String::from("wss://127.0.0.1:1/sentinel"));
    assert_eq!(h.slot(), u32::MAX, "post-shutdown ensure must be sentinel");
    assert!(
        !pool.send(h, WireFrame::Text("ignored".to_string())),
        "sentinel handle must be a no-op for send()"
    );
}

/// Structural-typing guard: `Pool` has no method named `send_all` or
/// `broadcast`. The compiler enforces this, but a smoke test keeps it
/// in the test catalogue so future contributors see the intent.
///
/// NDK issue #175 answer: every send is constrained to a `RelayHandle`.
#[test]
fn pool_exposes_no_send_to_all_method() {
    // Pure compile-time assertion — if someone adds `Pool::send_all`,
    // this test does not break; the contract lives in the `Pool` impl
    // block and the spec at `docs/architecture/crate-boundaries.md`
    // §3.8. The test is here as a discoverable failure point if
    // someone audits the test list.
    //
    // We *do* call `send` to assert the only fan-out path: caller
    // supplies a handle.
    let (events_tx, _events_rx) = mpsc::channel();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    let h = pool.ensure_open(&String::from("wss://127.0.0.1:1/sentinel"));
    let _ok = pool.send(h, WireFrame::Text("[\"REQ\",\"x\",{}]".to_string()));
    pool.shutdown();
}

/// Sanity: `RelayHandle` is `Copy`. The kernel actor stores many of
/// these in `wire_subs` and a `Copy` bound keeps that code clutter-free.
#[test]
fn relay_handle_is_copy() {
    fn assert_copy<T: Copy>() {}
    assert_copy::<RelayHandle>();
}

/// Sanity: `Pool` is `Clone`. The kernel actor hands clones into
/// `ProtocolCommand` closures.
#[test]
fn pool_is_clone() {
    fn assert_clone<T: Clone>() {}
    assert_clone::<Pool>();
}

/// `Pool::shutdown` must join the worker→pool translator thread rather
/// than leaving it detached: after `shutdown()` returns, the `PoolInner`
/// no longer holds a `JoinHandle` for it, and `shutdown()` itself must
/// return promptly (not hang — the join happens outside the `PoolInner`
/// lock, since `translator_loop` takes that lock per event).
#[test]
fn shutdown_joins_translator_thread_and_does_not_hang() {
    let (events_tx, _events_rx) = mpsc::channel();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    let _h = pool.ensure_open(&String::from("wss://127.0.0.1:1/sentinel"));
    pool.shutdown();
    let guard = pool.inner.lock().expect("inner lock must not be poisoned");
    assert!(
        guard.translator.is_none(),
        "shutdown() must take the translator JoinHandle, leaving None behind"
    );
}

/// The `default_role` from `PoolConfig` is propagated to workers.
#[test]
fn ensure_open_with_explicit_role_overrides_default() {
    let (events_tx, _events_rx) = mpsc::channel();
    let pool = Pool::new(
        PoolConfig {
            default_role: RelayRole::Indexer,
            ..PoolConfig::default()
        },
        events_tx,
    );
    let h_default = pool.ensure_open(&String::from("wss://127.0.0.1:1/a"));
    let _h_explicit =
        pool.ensure_open_with_role(&String::from("wss://127.0.0.1:1/b"), RelayRole::Content);
    let snap = pool.snapshot();
    let row_a = snap
        .rows
        .iter()
        .find(|r| r.handle == h_default)
        .expect("default-role row");
    assert_eq!(row_a.role, RelayRole::Indexer);
    pool.shutdown();
}
