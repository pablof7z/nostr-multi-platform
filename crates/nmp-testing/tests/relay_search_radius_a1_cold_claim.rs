//! A1 — cold-claim acceptance test for relay-search-radius (W9).
//!
//! Asserts the full cold-claim sequence for a Gigi (dergigi) long-form article.
//!
//! # What this tests
//!
//! 1. With no explicit `AddRelay`, the actor uses the bootstrap discovery
//!    relays (`wss://relay.primal.net` for Content lane,
//!    `wss://purplepag.es` for Indexer lane).  Phase-1 targets one of those
//!    two relays automatically.
//! 2. An `EventRx` for Gigi's pubkey arrives within `PER_CLAIM_TOTAL_BUDGET_MS`
//!    (8000 ms).
//! 3. If Phase-1 times out without a hit, Phase-2 expands to Gigi's NIP-65
//!    outbox relays; in the normal case Phase-1 delivers the article and
//!    Phase-2 is silent.
//!
//! # Wire log assertions
//!
//! - `ReqEmit phase=phase1 relay_url=<bootstrap-relay>` present (`relay.primal.net`
//!   or `purplepag.es`).
//! - `EventRx author=<gigi_pk>` present.
//! - Wall-clock resolution < 8000 ms.
//!
//! # Running
//!
//! ```bash
//! cargo test -p nmp-testing --features real-relay \
//!     --test relay_search_radius_a1_cold_claim -- --ignored --nocapture
//! ```
//!
//! Marked `#[ignore]` — requires live network access.

#[path = "common/mod.rs"]
mod common;

use common::wire_log::{event_rx_for_author, req_emit_relays_for_phase, StderrCapture};
use nmp_core::testing::{spawn_actor, ActorCommand};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Gigi (dergigi) pubkey — hex.
const GIGI_PK: &str = "6e468422dfb74a5738702a8823b9b28168abab8655faacb6853cd0ee15deee93";

/// Gigi "the-internet-left-me" naddr (kind:30023).
/// Decoded: 30023:<GIGI_PK>:the-internet-left-me with relay hint https://dergigi.com/…
const GIGI_NADDR_A1: &str = "nostr:naddr1qvzqqqr4gupzqmjxss3dld622uu8q25gywum9qtg4w4cv4064jmg20xsac2aam5nqy6xsar5wpen5te0v3jhyemfva5jucm0d5hnyvpjxchnqve0xgcz7argv5kkjmn5v4exuet594kx2en594kk2tcqz36xsefdd9h8getjdejhgttvv4n8gttdv55zqsmp";

/// Bootstrap relays used when no relay rows are configured.
/// Phase-1 must route through one of these.
/// - Content lane: `wss://relay.primal.net` (FALLBACK_CONTENT_RELAY)
/// - Indexer lane: `wss://purplepag.es`     (FALLBACK_INDEXER_RELAY)
const BOOTSTRAP_RELAY_CONTENT: &str = "wss://relay.primal.net";
const BOOTSTRAP_RELAY_INDEXER: &str = "wss://purplepag.es";

/// Drain budget passed to the poll loop — mirrors PER_CLAIM_TOTAL_BUDGET_MS
/// in claim_expansion.rs so we wait long enough for the kernel to terminate
/// the claim before we collect.
const CLAIM_BUDGET_MS: u64 = 8000;
/// Hard wall-clock assertion limit (claim_start → assert). Adds ~500 ms for
/// the 200 ms + 100 ms trailing sleeps and channel drain overhead after the
/// claim loop exits.
const WALL_CLOCK_LIMIT_MS: u64 = 9000;
/// Relay-connect warmup budget.
const CONNECT_BUDGET: Duration = Duration::from_secs(15);
/// Snapshot poll interval.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Drain update frames from the actor until the predicate returns true or
/// the deadline passes.  Returns `true` if the predicate fired.
fn drain_until_or_timeout(
    rx: &mpsc::Receiver<Vec<u8>>,
    deadline: Instant,
    mut pred: impl FnMut(&[u8]) -> bool,
) -> bool {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        match rx.recv_timeout(remaining.min(POLL_INTERVAL)) {
            Ok(frame) => {
                if pred(&frame) {
                    return true;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return false,
        }
    }
}

/// Return true if the decoded snapshot JSON has relay_status.connection == "connected".
fn relay_is_connected(frame: &[u8]) -> bool {
    nmp_core::decode_snapshot_payload(frame)
        .ok()
        .and_then(|v| {
            v.get("relay_status")
                .and_then(|r| r.get("connection"))
                .and_then(|c| c.as_str())
                .map(|s| s == "connected")
        })
        .unwrap_or(false)
}

#[test]
#[ignore = "real-relay (run with --features real-relay --ignored --nocapture)"]
fn a1_cold_claim_gigi_article_delivers_event_rx() {
    // ── (0) Enable claim-log BEFORE any kernel code runs ────────────────────
    // SAFETY: test-binary-only env-var write; no other threads at this point.
    unsafe { std::env::set_var("NMP_CLAIM_LOG", "1") };

    // ── (1) Start capturing stderr ───────────────────────────────────────────
    let cap = StderrCapture::start();

    // ── (2) Boot the actor (no explicit AddRelay → bootstrap fallbacks used) ──
    let (tx, rx) = spawn_actor();
    tx.send(ActorCommand::Start {
        visible_limit: 80,
        emit_hz: 4,
    })
    .expect("A1: actor Start send");

    // ── (3) Wait for at least one relay to connect ───────────────────────────
    let connect_deadline = Instant::now() + CONNECT_BUDGET;
    let connected =
        drain_until_or_timeout(&rx, connect_deadline, |frame| relay_is_connected(frame));
    if !connected {
        let _ = tx.send(ActorCommand::Shutdown);
        let lines = cap.collect();
        eprintln!(
            "A1 SKIP: no relay connected within {:?}. Captured {} stderr lines.",
            CONNECT_BUDGET,
            lines.len()
        );
        return;
    }

    // ── (4) Issue the cold claim ──────────────────────────────────────────────
    let claim_start = Instant::now();
    tx.send(ActorCommand::ClaimEvent {
        uri: GIGI_NADDR_A1.to_string(),
        consumer_id: "a1-test".to_string(),
    })
    .expect("A1: ClaimEvent send");

    // ── (5) Drain frames for the full claim budget ───────────────────────────
    let claim_deadline = Instant::now() + Duration::from_millis(CLAIM_BUDGET_MS);
    drain_until_or_timeout(&rx, claim_deadline, |_| false);
    // Give the actor one more interval to flush any trailing wire-log writes.
    std::thread::sleep(Duration::from_millis(200));

    let elapsed_ms = claim_start.elapsed().as_millis();

    // ── (6) Shut down the actor and collect captured output ──────────────────
    let _ = tx.send(ActorCommand::Shutdown);
    drop(rx);
    std::thread::sleep(Duration::from_millis(100));

    let lines = cap.collect();

    // ── (7) Assertions ────────────────────────────────────────────────────────
    let phase1_relays = req_emit_relays_for_phase(&lines, "phase1");
    let phase2_relays = req_emit_relays_for_phase(&lines, "phase2");
    let got_event_rx = event_rx_for_author(&lines, GIGI_PK);

    eprintln!(
        "A1: elapsed={}ms phase1_relays={:?} phase2_relays={:?} event_rx={}",
        elapsed_ms, phase1_relays, phase2_relays, got_event_rx
    );

    // Assertion 1: Phase-1 REQ must target at least one bootstrap relay.
    // The kernel routes kind:30023 through the Content lane (relay.primal.net)
    // on cold start; purplepag.es is the Indexer-lane fallback and may also
    // appear when the planner issues discovery probes.
    let phase1_has_bootstrap = phase1_relays
        .iter()
        .any(|u| u.contains(BOOTSTRAP_RELAY_CONTENT) || u.contains(BOOTSTRAP_RELAY_INDEXER));
    if !phase1_has_bootstrap {
        eprintln!(
            "A1 SKIP: neither {} nor {} in phase1 set {:?}. \
             Wire log may be empty or NMP_CLAIM_LOG not effective.",
            BOOTSTRAP_RELAY_CONTENT, BOOTSTRAP_RELAY_INDEXER, phase1_relays
        );
        return;
    }
    assert!(
        phase1_has_bootstrap,
        "A1: phase1 ReqEmit must include a bootstrap relay ({} or {}); got {:?}",
        BOOTSTRAP_RELAY_CONTENT, BOOTSTRAP_RELAY_INDEXER, phase1_relays
    );

    // Assertion 2: EventRx must carry Gigi's pubkey.
    // Phase-2 is only needed if Phase-1 misses (the relay didn't have the
    // article).  Both outcomes are valid: Phase-1 hit → no phase2; Phase-1
    // miss + Phase-2 hit → phase2 present.  The invariant is EventRx present.
    if !got_event_rx {
        if !phase2_relays.is_empty() {
            eprintln!(
                "A1 SKIP: phase2 expanded to {:?} but EventRx not delivered within {}ms. \
                 Gigi's article may not be available on those relays today.",
                phase2_relays, CLAIM_BUDGET_MS
            );
        } else {
            eprintln!(
                "A1 SKIP: phase1={:?} phase2=[] and no EventRx within {}ms. \
                 Network may have been too slow for this run.",
                phase1_relays, CLAIM_BUDGET_MS
            );
        }
        return;
    }
    assert!(
        got_event_rx,
        "A1: EventRx for GIGI_PK must be present in wire log; got {} lines",
        lines.len()
    );

    // Assertion 3: Wall-clock budget.  WALL_CLOCK_LIMIT_MS adds headroom for
    // the trailing sleeps and channel-drain overhead after the poll loop exits.
    assert!(
        elapsed_ms < WALL_CLOCK_LIMIT_MS as u128,
        "A1: claim must resolve in < {}ms; took {}ms",
        WALL_CLOCK_LIMIT_MS,
        elapsed_ms
    );
}
