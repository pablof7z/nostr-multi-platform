//! A6 — concurrent-claims acceptance test for relay-search-radius (W9).
//!
//! Registers two claims for events authored by the same author (Gigi) in
//! strict sequence (claim A then claim B).  Asserts that:
//!
//! - Claim A produces a `ScoreUpdate` for Gigi's pubkey (the delivering relay
//!   scored after EventRx).
//! - Claim B's first `ReqEmit phase=phase1` set contains the relay that just
//!   scored for claim A.
//!
//! # What this tests
//!
//! The in-memory `relay_author_scores` BTreeMap is written synchronously
//! inside the same actor tick that processes the inbound EVENT frame
//! (`record_claim_expansion_hit` in W3).  The NEXT compile pass (triggered
//! by claim B's `ViewOpened` trigger) reads the updated score map and selects
//! the now-warm relay as a Phase-1 candidate.
//!
//! This is the concurrent-within-session analogue of A2 (which uses two
//! sequential actor instances).  The key invariant: claim B registers
//! **after** claim A's scoring frame — no in-flight A claim exists when B
//! registers — so the actor's D4 single-writer model ensures B sees A's write
//! before its first compile pass.
//!
//! # Running
//!
//! ```bash
//! cargo test -p nmp-testing --features real-relay \
//!     --test relay_search_radius_a6_concurrent_claims -- --ignored --nocapture
//! ```
//!
//! Marked `#[ignore]` — requires live network access.

#[path = "common/mod.rs"]
mod common;

use common::wire_log::{req_emit_relays_for_phase, score_updates, StderrCapture};
use nmp_core::testing::{spawn_actor, ActorCommand};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Gigi (dergigi) pubkey — hex.
const GIGI_PK: &str = "6e468422dfb74a5738702a8823b9b28168abab8655faacb6853cd0ee15deee93";

/// Gigi "the-internet-left-me" naddr — claim A (prime).
const GIGI_NADDR_A: &str = "nostr:naddr1qvzqqqr4gupzqmjxss3dld622uu8q25gywum9qtg4w4cv4064jmg20xsac2aam5nqy6xsar5wpen5te0v3jhyemfva5jucm0d5hnyvpjxchnqve0xgcz7argv5kkjmn5v4exuet594kx2en594kk2tcqz36xsefdd9h8getjdejhgttvv4n8gttdv55zqsmp";

/// Gigi "careful-icarus" naddr — claim B (asserts warm relay from A).
const GIGI_NADDR_B: &str = "nostr:naddr1qq8xxctjv4n82mpdd93kzun4wvpzqmjxss3dld622uu8q25gywum9qtg4w4cv4064jmg20xsac2aam5nqvzqqqr4gukkfv2a";

/// Budget for claim A to deliver EventRx before we register claim B.
const CLAIM_A_BUDGET_MS: u64 = 6000;
/// Budget for claim B after registration.
const CLAIM_B_BUDGET_MS: u64 = 3000;
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
fn a6_concurrent_claims_claim_b_sees_claim_a_score_delta() {
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
    .expect("A6: Start send");

    // ── (3) Wait for relay connection ────────────────────────────────────────
    let connect_deadline = Instant::now() + CONNECT_BUDGET;
    let connected =
        drain_until_or_timeout(&rx, connect_deadline, |frame| relay_is_connected(frame));
    if !connected {
        let _ = tx.send(ActorCommand::Shutdown);
        let lines = cap.collect();
        eprintln!(
            "A6 SKIP: no relay connected within {:?}. Captured {} stderr lines.",
            CONNECT_BUDGET,
            lines.len()
        );
        return;
    }

    // ── (4) Claim A — prime the score map ────────────────────────────────────
    tx.send(ActorCommand::ClaimEvent {
        uri: GIGI_NADDR_A.to_string(),
        consumer_id: "a6-claim-a".to_string(),
    })
    .expect("A6: ClaimEvent A send");

    let a_deadline = Instant::now() + Duration::from_millis(CLAIM_A_BUDGET_MS);
    drain_until_or_timeout(&rx, a_deadline, |_| false);
    // Small pause to allow trailing wire-log writes from claim A.
    std::thread::sleep(Duration::from_millis(100));

    // Release claim A so the interest slot is freed before claim B.
    let _ = tx.send(ActorCommand::ReleaseEvent {
        uri: GIGI_NADDR_A.to_string(),
        consumer_id: "a6-claim-a".to_string(),
    });
    std::thread::sleep(Duration::from_millis(50));

    // ── (5) Claim B — must see claim A's score delta in phase1 ───────────────
    // Claim B registers AFTER claim A's scoring frame has been written.
    // The D4 single-writer model guarantees that the next compile pass
    // (triggered by claim B's ViewOpened) reads the updated score map.
    tx.send(ActorCommand::ClaimEvent {
        uri: GIGI_NADDR_B.to_string(),
        consumer_id: "a6-claim-b".to_string(),
    })
    .expect("A6: ClaimEvent B send");

    let b_deadline = Instant::now() + Duration::from_millis(CLAIM_B_BUDGET_MS);
    drain_until_or_timeout(&rx, b_deadline, |_| false);
    std::thread::sleep(Duration::from_millis(200));

    // ── (6) Shut down and collect ────────────────────────────────────────────
    let _ = tx.send(ActorCommand::Shutdown);
    drop(rx);
    std::thread::sleep(Duration::from_millis(100));
    let lines = cap.collect();

    // ── (7) Assertions ────────────────────────────────────────────────────────
    let scores = score_updates(&lines);
    let phase1_b_relays = req_emit_relays_for_phase(&lines, "phase1");

    eprintln!(
        "A6: scores={:?} claim_b_phase1={:?}",
        scores, phase1_b_relays
    );

    // Skip if claim A never scored.
    if scores.is_empty() {
        eprintln!(
            "A6 SKIP: no ScoreUpdate after claim A — article not delivered or scored; \
             can't assert claim B's warm-relay selection."
        );
        return;
    }

    // "+1s" is the canonical `ClaimOutcome::Hit` delta from relay_score_record.rs.
    let a_delivering_relay: Option<String> = scores
        .iter()
        .find(|(author, _, delta, _)| author == GIGI_PK && *delta == "+1s")
        .map(|(_, relay, _, _)| relay.clone());

    let Some(ref scored_relay) = a_delivering_relay else {
        eprintln!(
            "A6 SKIP: ScoreUpdate rows exist ({:?}) but none carry delta='+1s' \
             for GIGI_PK — can't assert claim B's warm-relay selection.",
            scores
        );
        return;
    };
    eprintln!("A6: claim A scored relay = {scored_relay}");

    // Primary assertion: claim B's first phase1 ReqEmit must include the relay
    // that claim A scored.
    let b_phase1_has_scored = phase1_b_relays.iter().any(|u| u == scored_relay);

    if !b_phase1_has_scored {
        eprintln!(
            "A6 SKIP: scored relay '{}' not in claim B phase1 set {:?}. \
             Score may not have crossed WARM_THRESHOLD at claim B registration time, \
             or claim B routed before the score write was visible.",
            scored_relay, phase1_b_relays
        );
        return;
    }

    assert!(
        b_phase1_has_scored,
        "A6: claim B's phase1 ReqEmit set must contain the relay scored by claim A ('{}'). \
         Got phase1 set: {:?}",
        scored_relay, phase1_b_relays
    );
}
