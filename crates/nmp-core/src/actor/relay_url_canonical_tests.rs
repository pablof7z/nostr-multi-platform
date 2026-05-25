//! T-relay-url-normalize — actor-level canonicalization integration tests.
//!
//! Pins that `ensure_relay_worker` and `shutdown_relay_worker` agree on pool
//! keys after canonicalization (case/trailing-slash variants treated as one),
//! and that add-then-remove with differing URL forms actually shuts the worker
//! down without leaking.
//!
//! Phase F: tests construct a real [`nmp_network::pool::Pool`] (the actor's
//! transport substrate post-cut-over) and assert the same pool-bookkeeping
//! invariants the pre-Pool design exposed.

use super::relay_mgmt::{close_relays, ensure_relay_worker, shutdown_relay_worker};
use super::RelayControl;
use crate::kernel::Kernel;
use crate::relay::{CanonicalRelayUrl, RelayRole};
use nmp_network::pool::{Pool, PoolConfig, PoolEvent};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;

/// Build the actor-side transport state every test in this file needs:
/// a fresh `Pool` (wrapping a real worker pool), the event receiver
/// (kept around so the channel doesn't disconnect mid-test), and the
/// empty `relay_controls` + `slot_to_url` side-maps.
fn fresh_pool() -> (
    Pool,
    mpsc::Receiver<PoolEvent>,
    HashMap<CanonicalRelayUrl, RelayControl>,
    HashMap<u32, CanonicalRelayUrl>,
) {
    let (events_tx, events_rx) = mpsc::channel::<PoolEvent>();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    (pool, events_rx, HashMap::new(), HashMap::new())
}

// ── Pool dedup — canonical key equality ──────────────────────────────────────

/// T-normalize-1: `wss://R.Ex/` and `wss://r.ex` must resolve to the same
/// canonical key and share a single pool entry.
///
/// Before T-relay-url-normalize, `ensure_relay_worker` keyed by raw bytes so
/// the two forms would spawn two workers — causing duplicate sockets.
#[test]
fn t_normalize_case_and_slash_variants_share_one_pool_entry() {
    let mut kernel = Kernel::new(80);
    let (pool, _events_rx, mut relay_controls, mut slot_to_url) = fresh_pool();
    let mut next_gen = 1_u64;

    let spawned_a = ensure_relay_worker(
        &mut relay_controls,
        &mut slot_to_url,
        &pool,
        &mut kernel,
        &mut next_gen,
        RelayRole::Content,
        "wss://R.Ex/".to_string(),
    );
    let spawned_b = ensure_relay_worker(
        &mut relay_controls,
        &mut slot_to_url,
        &pool,
        &mut kernel,
        &mut next_gen,
        RelayRole::Content,
        "wss://r.ex".to_string(),
    );

    assert!(spawned_a, "first ensure_relay_worker must spawn a worker");
    assert!(
        !spawned_b,
        "second call with URL-equivalent form must NOT spawn a second worker"
    );
    assert_eq!(
        relay_controls.len(),
        1,
        "T-normalize-1: URL-equivalent forms must share one pool entry, got {}",
        relay_controls.len()
    );

    let mut connected = HashSet::new();
    close_relays(
        &mut relay_controls,
        &mut slot_to_url,
        &pool,
        &mut connected,
        &mut kernel,
    );
}

/// T-normalize-2: uppercase-scheme variant (`WSS://R.Ex`) maps to the same
/// canonical key as the lowercase form.
#[test]
fn t_normalize_uppercase_scheme_deduplicates() {
    let mut kernel = Kernel::new(80);
    let (pool, _events_rx, mut relay_controls, mut slot_to_url) = fresh_pool();
    let mut next_gen = 1_u64;

    let spawned_a = ensure_relay_worker(
        &mut relay_controls,
        &mut slot_to_url,
        &pool,
        &mut kernel,
        &mut next_gen,
        RelayRole::Content,
        "WSS://R.Ex".to_string(),
    );
    let spawned_b = ensure_relay_worker(
        &mut relay_controls,
        &mut slot_to_url,
        &pool,
        &mut kernel,
        &mut next_gen,
        RelayRole::Indexer,
        "wss://r.ex".to_string(),
    );

    assert!(spawned_a, "first call must spawn");
    assert!(
        !spawned_b,
        "lowercase form must hit the canonical key of the first call"
    );
    assert_eq!(
        relay_controls.len(),
        1,
        "T-normalize-2: one entry for case-variant forms"
    );

    let mut connected = HashSet::new();
    close_relays(
        &mut relay_controls,
        &mut slot_to_url,
        &pool,
        &mut connected,
        &mut kernel,
    );
}

/// T-normalize-3: relay with a real non-empty path is distinct from the root
/// form. `wss://r.ex/nostr` must NOT merge with `wss://r.ex`.
#[test]
fn t_normalize_nonempty_path_is_distinct() {
    let mut kernel = Kernel::new(80);
    let (pool, _events_rx, mut relay_controls, mut slot_to_url) = fresh_pool();
    let mut next_gen = 1_u64;

    let spawned_a = ensure_relay_worker(
        &mut relay_controls,
        &mut slot_to_url,
        &pool,
        &mut kernel,
        &mut next_gen,
        RelayRole::Content,
        "wss://r.ex".to_string(),
    );
    let spawned_b = ensure_relay_worker(
        &mut relay_controls,
        &mut slot_to_url,
        &pool,
        &mut kernel,
        &mut next_gen,
        RelayRole::Content,
        "wss://r.ex/nostr".to_string(),
    );

    assert!(spawned_a, "root form must spawn");
    assert!(
        spawned_b,
        "non-empty-path form is distinct and must also spawn"
    );
    assert_eq!(
        relay_controls.len(),
        2,
        "T-normalize-3: two distinct pool entries for root vs non-empty-path relay"
    );

    let mut connected = HashSet::new();
    close_relays(
        &mut relay_controls,
        &mut slot_to_url,
        &pool,
        &mut connected,
        &mut kernel,
    );
}

// ── Add/remove round-trip — no worker leak ───────────────────────────────────

/// T-normalize-4: add via `wss://R.Ex/` then remove via `wss://r.ex` must
/// actually shut the worker down (no socket leak).
///
/// Before the fix, add and remove used different canonical forms — the remove
/// would not find the pool entry and the worker would leak.
///
/// This test does NOT wait for a Connected event (the loopback URL will fail
/// DNS/TCP) — it only asserts synchronous pool accounting.
#[test]
fn t_normalize_add_uppercase_remove_lowercase_no_leak() {
    let mut kernel = Kernel::new(80);
    let (pool, _events_rx, mut relay_controls, mut slot_to_url) = fresh_pool();
    let mut next_gen = 1_u64;

    // Add with uppercase + trailing slash.
    let spawned = ensure_relay_worker(
        &mut relay_controls,
        &mut slot_to_url,
        &pool,
        &mut kernel,
        &mut next_gen,
        RelayRole::Content,
        "WSS://127.0.0.1:1/".to_string(),
    );
    assert!(spawned, "add must spawn a worker");
    assert_eq!(
        relay_controls.len(),
        1,
        "pool must have one entry after add"
    );

    // Remove with lowercase + no trailing slash (the canonical form).
    let removed = shutdown_relay_worker(
        &mut relay_controls,
        &mut slot_to_url,
        &pool,
        "wss://127.0.0.1:1",
    );
    assert!(
        removed,
        "T-normalize-4: shutdown_relay_worker must find the canonical key and return true"
    );
    assert!(
        relay_controls.is_empty(),
        "T-normalize-4: pool must be empty after remove — no worker leak"
    );
}

/// T-normalize-5: add via `wss://r.ex` then remove via `wss://r.ex/` (trailing
/// slash variant) must also shut the worker down cleanly.
#[test]
fn t_normalize_add_lowercase_remove_trailing_slash_no_leak() {
    let mut kernel = Kernel::new(80);
    let (pool, _events_rx, mut relay_controls, mut slot_to_url) = fresh_pool();
    let mut next_gen = 1_u64;

    let spawned = ensure_relay_worker(
        &mut relay_controls,
        &mut slot_to_url,
        &pool,
        &mut kernel,
        &mut next_gen,
        RelayRole::Content,
        "wss://127.0.0.1:1".to_string(),
    );
    assert!(spawned, "add must spawn a worker");

    // Remove with the trailing-slash variant.
    let removed = shutdown_relay_worker(
        &mut relay_controls,
        &mut slot_to_url,
        &pool,
        "wss://127.0.0.1:1/",
    );
    assert!(
        removed,
        "T-normalize-5: shutdown_relay_worker with trailing-slash variant must return true"
    );
    assert!(
        relay_controls.is_empty(),
        "T-normalize-5: pool must be empty — no worker leak"
    );
}
