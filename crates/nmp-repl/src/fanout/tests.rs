use std::collections::BTreeMap;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;

use super::{launch, run_discovery, ContentReq, RelayStats};

fn req(relay: &str, sub: &str, authors: usize) -> ContentReq {
    ContentReq {
        relay: relay.to_string(),
        sub_id: sub.to_string(),
        filter_json: "{\"kinds\":[1]}".to_string(),
        authors,
    }
}

// ── RelayStats invariants ────────────────────────────────────────────

#[test]
fn relay_stats_default_is_empty() {
    let s = RelayStats::default();
    assert_eq!(s.events, 0);
    assert_eq!(s.authors_in_req, 0);
    assert!(s.time_to_first.is_none());
    assert!(!s.connected);
    assert!(!s.eose);
    assert!(s.error.is_none());
    assert!(s.elapsed.is_none());
}

// ── launch: empty + invalid-scheme inputs (no network) ───────────────

#[test]
fn launch_empty_map_clamps_workers_to_one_and_disconnects() {
    // No jobs at all: total_jobs=0 → workers clamped to max(1)=1, and the
    // dropped work channel makes that worker exit immediately. The event
    // receiver must then observe a clean disconnect (no panic, no hang).
    let per_relay: BTreeMap<String, Vec<ContentReq>> = BTreeMap::new();
    let (rx, workers, _deadline) = launch(&per_relay, Duration::from_millis(50));
    assert_eq!(workers, 1, "worker count clamps to a floor of 1");
    // Every sender is dropped once the (only) worker exits → Disconnected.
    // A bounded wait keeps the test from hanging if that ever regresses.
    match rx.recv_timeout(Duration::from_secs(2)) {
        Err(mpsc::RecvTimeoutError::Disconnected) => {}
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("worker never exited — channel should disconnect promptly")
        }
        Ok(ev) => panic!("expected no events from an empty plan, got {ev:?}"),
    }
}

#[test]
fn launch_filters_out_non_ws_scheme_urls() {
    // Only non-ws URLs: every job is filtered → total_jobs=0 → workers=1,
    // and no thread ever dials anything. Confirms the scheme guard in
    // `launch` keeps `http`/`ftp`/bare-host entries off the wire.
    let mut per_relay: BTreeMap<String, Vec<ContentReq>> = BTreeMap::new();
    per_relay.insert(
        "http://relay.example".to_string(),
        vec![req("http://relay.example", "s1", 1)],
    );
    per_relay.insert(
        "ftp://relay.example".to_string(),
        vec![req("ftp://relay.example", "s2", 1)],
    );
    per_relay.insert(
        "relay.example".to_string(),
        vec![req("relay.example", "s3", 1)],
    );
    let (rx, workers, _deadline) = launch(&per_relay, Duration::from_millis(50));
    assert_eq!(workers, 1, "all non-ws jobs filtered → worker floor of 1");
    // No job queued → the lone worker exits immediately, channel closes.
    assert!(
        matches!(
            rx.recv_timeout(Duration::from_secs(2)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ),
        "no relay events when every URL is filtered"
    );
}

#[test]
fn launch_returns_future_deadline() {
    let per_relay: BTreeMap<String, Vec<ContentReq>> = BTreeMap::new();
    let wall = Duration::from_secs(10);
    let before = Instant::now();
    let (_rx, _workers, deadline) = launch(&per_relay, wall);
    // The deadline is `now + wall`; it must lie in the future window.
    assert!(deadline > before);
    assert!(deadline <= Instant::now() + wall);
}

// ── run_discovery: empty input (no network) ──────────────────────────

#[test]
fn run_discovery_empty_probes_returns_empty() {
    // No probes → empty `by_relay` → the relay loop never runs → no
    // socket is ever opened. Pure, deterministic, network-free.
    let out = run_discovery(&[]);
    assert!(out.is_empty());
}

// ── ContentReq / RelayEvent are constructible + cloneable ────────────

#[test]
fn content_req_clone_preserves_fields() {
    let r = req("wss://relay.example", "sub-1", 42);
    let c = r.clone();
    assert_eq!(c.relay, "wss://relay.example");
    assert_eq!(c.sub_id, "sub-1");
    assert_eq!(c.authors, 42);
    assert_eq!(c.filter_json, "{\"kinds\":[1]}");
}
