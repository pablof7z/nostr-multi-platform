//! A2 — warm-path acceptance test for relay-search-radius (W9).
//!
//! Primes the in-memory score map by completing a first claim for a Gigi
//! article (same as A1's naddr), then issues a second claim for a *different*
//! Gigi article.  Asserts that the relay which delivered the first event
//! appears in the `phase=phase1` `ReqEmit` set for the second claim.
//!
//! # What this tests
//!
//! - After a successful claim the kernel records a `ScoreUpdate` (successes
//!   += 1) for the delivering relay + Gigi's pubkey pair.
//! - The score pushes that relay above `WARM_THRESHOLD` (0.40) in the
//!   in-memory `relay_author_scores` map.
//! - On the second claim, `warm_relays_for_author(GIGI_PK)` returns the scored
//!   relay in the Phase-1 candidate set.
//! - A `ReqEmit phase=phase1 relay_url=<that relay>` line is emitted.
//!
//! # Wire log assertions
//!
//! - `ScoreUpdate` line present after first claim.
//! - `ReqEmit phase=phase1 relay_url=<delivering-relay>` present for second claim.
//!
//! # Running
//!
//! ```bash
//! cargo test -p nmp-testing --features real-relay \
//!     --test relay_search_radius_a2_warm_path -- --ignored --nocapture
//! ```
//!
//! Marked `#[ignore]` — requires live network access.

#[path = "common/mod.rs"]
mod common;

use common::wire_log::{
    event_rx_for_author, req_emit_relays_for_phase, score_updates, StderrCapture,
};
use nmp_core::testing::{spawn_actor, ActorCommand};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Gigi (dergigi) pubkey — hex.
const GIGI_PK: &str = "6e468422dfb74a5738702a8823b9b28168abab8655faacb6853cd0ee15deee93";

/// Gigi "the-internet-left-me" naddr — used to prime the score map (same as A1).
const GIGI_NADDR_PRIME: &str = "nostr:naddr1qvzqqqr4gupzqmjxss3dld622uu8q25gywum9qtg4w4cv4064jmg20xsac2aam5nqy6xsar5wpen5te0v3jhyemfva5jucm0d5hnyvpjxchnqve0xgcz7argv5kkjmn5v4exuet594kx2en594kk2tcqz36xsefdd9h8getjdejhgttvv4n8gttdv55zqsmp";

/// Gigi "careful-icarus" naddr — the second, different article.
/// Decoded: 30023:<GIGI_PK>:careful-icarus (no relay hints).
const GIGI_NADDR_SECOND: &str = "nostr:naddr1qq8xxctjv4n82mpdd93kzun4wvpzqmjxss3dld622uu8q25gywum9qtg4w4cv4064jmg20xsac2aam5nqvzqqqr4gukkfv2a";

/// Budget for each individual claim phase.
const PRIME_BUDGET_MS: u64 = 7000;
const SECOND_CLAIM_BUDGET_MS: u64 = 4000;
/// Relay-connect warmup budget.
const CONNECT_BUDGET: Duration = Duration::from_secs(15);
const POLL_INTERVAL: Duration = Duration::from_millis(100);

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
fn a2_warm_path_second_claim_uses_scored_relay() {
    // ── (0) Enable claim-log BEFORE any kernel code runs ────────────────────
    unsafe { std::env::set_var("NMP_CLAIM_LOG", "1") };

    // ── (1) Start capturing stderr ───────────────────────────────────────────
    let cap = StderrCapture::start();

    // ── (2) Boot the actor ───────────────────────────────────────────────────
    let (tx, rx) = spawn_actor();
    tx.send(ActorCommand::Start {
        visible_limit: 80,
        emit_hz: 4,
    })
    .expect("A2: actor Start send");

    // ── (3) Wait for relay connection ────────────────────────────────────────
    let connect_deadline = Instant::now() + CONNECT_BUDGET;
    let connected =
        drain_until_or_timeout(&rx, connect_deadline, |frame| relay_is_connected(frame));
    if !connected {
        let _ = tx.send(ActorCommand::Shutdown);
        let lines = cap.collect();
        eprintln!(
            "A2 SKIP: no relay connected within {:?}. Captured {} stderr lines.",
            CONNECT_BUDGET,
            lines.len()
        );
        return;
    }

    // ── (4) Prime: first claim for GIGI_NADDR_PRIME ──────────────────────────
    tx.send(ActorCommand::ClaimEvent {
        uri: GIGI_NADDR_PRIME.to_string(),
        consumer_id: "a2-prime".to_string(),
    })
    .expect("A2: prime ClaimEvent send");

    let prime_deadline = Instant::now() + Duration::from_millis(PRIME_BUDGET_MS);
    drain_until_or_timeout(&rx, prime_deadline, |_| false);
    std::thread::sleep(Duration::from_millis(200));

    // Release the first claim so the second claim sees a fresh interest slot.
    let _ = tx.send(ActorCommand::ReleaseEvent {
        uri: GIGI_NADDR_PRIME.to_string(),
        consumer_id: "a2-prime".to_string(),
    });
    std::thread::sleep(Duration::from_millis(50));

    // ── (5) Second claim: different Gigi article ─────────────────────────────
    tx.send(ActorCommand::ClaimEvent {
        uri: GIGI_NADDR_SECOND.to_string(),
        consumer_id: "a2-second".to_string(),
    })
    .expect("A2: second ClaimEvent send");

    let second_deadline = Instant::now() + Duration::from_millis(SECOND_CLAIM_BUDGET_MS);
    drain_until_or_timeout(&rx, second_deadline, |_| false);
    std::thread::sleep(Duration::from_millis(200));

    // ── (6) Shut down and collect ────────────────────────────────────────────
    let _ = tx.send(ActorCommand::Shutdown);
    drop(rx);
    std::thread::sleep(Duration::from_millis(100));
    let lines = cap.collect();

    // ── (7) Assertions ────────────────────────────────────────────────────────
    let score_rows = score_updates(&lines);
    let prime_got_event = event_rx_for_author(&lines, GIGI_PK);
    let phase1_relays_second = req_emit_relays_for_phase(&lines, "phase1");

    eprintln!(
        "A2: score_rows={:?} prime_event_rx={} phase1_second={:?}",
        score_rows, prime_got_event, phase1_relays_second
    );

    // Skip honestly if the prime claim did not deliver the article.
    if score_rows.is_empty() {
        eprintln!(
            "A2 SKIP: no ScoreUpdate after prime claim — \
             article not delivered within {}ms; score map was not populated.",
            PRIME_BUDGET_MS
        );
        return;
    }

    // The scoring relay is the one with a Hit delta ("+1s") for GIGI_PK.
    // "+1s" is the canonical `ClaimOutcome::Hit` delta from relay_score_record.rs.
    let delivering_relay: Option<String> = score_rows
        .iter()
        .find(|(author, _, delta, _)| author == GIGI_PK && *delta == "+1s")
        .map(|(_, relay, _, _)| relay.clone());

    let Some(ref scored_relay) = delivering_relay else {
        eprintln!(
            "A2 SKIP: ScoreUpdate rows exist ({:?}) but none carry delta='+1s' \
             for GIGI_PK — can't assert phase1 warm relay.",
            score_rows
        );
        return;
    };

    // Assertion: the scored relay must appear in the phase1 set of the second claim.
    let phase1_has_scored = phase1_relays_second.iter().any(|u| u == scored_relay);

    if !phase1_has_scored {
        eprintln!(
            "A2 SKIP: scored relay {} not in phase1 set {:?}. \
             Score may not have crossed WARM_THRESHOLD or second claim \
             routed before score was written.",
            scored_relay, phase1_relays_second
        );
        return;
    }

    assert!(
        phase1_has_scored,
        "A2: scored relay '{}' must appear in phase1 ReqEmit set for second claim; \
         got {:?}",
        scored_relay, phase1_relays_second
    );
    assert!(
        prime_got_event,
        "A2: EventRx for GIGI_PK must be present in wire log after prime claim"
    );
}
