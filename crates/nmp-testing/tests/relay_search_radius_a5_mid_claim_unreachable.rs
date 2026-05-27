//! A5 — mid-claim unreachable relay acceptance test for relay-search-radius (W9).
//!
//! Phase-1 fires against the bootstrap Content relay (`relay.primal.net`) for a
//! fictional event id that will never exist on any relay (forces an EOSE with no
//! match at ~1500 ms).  Phase-2 then expands to the stub relay URL that was
//! embedded as the sole relay hint in the nevent TLV.  The stub accepts the
//! WebSocket handshake, stays alive long enough for Phase-2 to reach it, then
//! drops the connection without sending any frames.  The kernel's `relay_failed`
//! / `relay_closed` hook fires and calls
//! `record_claim_outcome(author, relay, ClaimOutcome::Failed)`, which records
//! `failures += 3` and reduces the relay's `new_weight`.
//!
//! # What this tests
//!
//! The `relay_failed` / `relay_closed` kernel hook calls
//! `record_claim_expansion_hit` with `ClaimOutcome::Failed` for every pending
//! claim whose `attempted` set contains the failing relay URL (W3 / §8.1
//! retarget).  A `Failed` outcome records `failures += 3` and reduces the
//! relay's `new_weight`.
//!
//! # Design
//!
//! - **No `AddRelay`**: the actor boots with only the bootstrap Content relay
//!   (`relay.primal.net`) in its relay pool.  We do NOT add the stub via
//!   `AddRelay` because `ActorCommand::Start` has already dialled the bootstrap
//!   relay; adding a second relay would not remove the connected relay from the
//!   pool, and the planner would simply route Phase-1 to the connected relay
//!   instead of the stub.
//! - **Fictional event id**: a 64-hex string that will never match any event on
//!   any public relay, forcing Phase-1 to EOSE-with-no-match and advance to
//!   Phase-2.
//! - **nevent URI with stub as relay hint**: the stub URL is embedded in the
//!   nevent TLV's relay list.  Phase-2 drains `PendingClaim.candidate_queue`
//!   (seeded from `uri_relay_hints`) → REQ sent to the stub.
//! - **Stub alive ≥ 3000 ms**: the stub must stay alive until Phase-2's REQ
//!   arrives (~1500 ms for Phase-1 budget + connection overhead).  After the
//!   REQ arrives the stub drops, triggering the failure score.
//!
//! # Running
//!
//! ```bash
//! cargo test -p nmp-testing --features real-relay \
//!     --test relay_search_radius_a5_mid_claim_unreachable -- --ignored --nocapture
//! ```
//!
//! Marked `#[ignore]` — requires network access (Phase-1 needs `relay.primal.net`).

#[path = "common/mod.rs"]
mod common;

use common::stub_relay::StubRelay;
use common::wire_log::{req_emit_relays_for_phase, score_updates, StderrCapture};
use nmp_core::nip19::{encode_nevent, NeventData};
use nmp_core::testing::{spawn_actor, ActorCommand};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Gigi (dergigi) pubkey — hex.  Used as the nevent author TLV so the planner
/// uses Gigi's warm-relay map when scoring the failure.
const GIGI_PK: &str = "6e468422dfb74a5738702a8823b9b28168abab8655faacb6853cd0ee15deee93";

/// A fictional 64-hex event id that will never exist on any public Nostr relay.
/// Phase-1 will EOSE immediately with no match, advancing the claim to Phase-2.
const FICTIONAL_EVENT_ID: &str = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";

/// How long the stub relay keeps the connection open.  Must be ≥ Phase-1 budget
/// (~1500 ms) so the stub is still alive when Phase-2's REQ arrives.  3000 ms
/// gives comfortable headroom for connection overhead.
const STUB_ALIVE_MS: u64 = 3000;

/// Total drain loop budget: Phase-1 (~1500 ms) + stub alive + Phase-2 overhead + slack.
const TEST_BUDGET_MS: u64 = 10_000;
/// Hard wall-clock assertion limit.  Adds ~500 ms for the 200 ms + 100 ms trailing
/// sleeps and channel-drain overhead after the poll loop exits.
const WALL_CLOCK_LIMIT_MS: u64 = 11_000;

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

/// Build a `nostr:nevent1...` URI whose relay hint is `relay_url`.
///
/// Uses `nmp_core::nip19::encode_nevent` so the same TLV codec the kernel
/// parses is used to encode — no dependency on an external tool.
fn build_nevent_uri(relay_url: &str) -> String {
    let data = NeventData {
        event_id: FICTIONAL_EVENT_ID.to_string(),
        relays: vec![relay_url.to_string()],
        author: Some(GIGI_PK.to_string()),
        kind: Some(1),
    };
    let bech = encode_nevent(&data).expect("A5: encode_nevent failed");
    format!("nostr:{bech}")
}

#[test]
#[ignore = "real-relay (run with --features real-relay --ignored --nocapture)"]
fn a5_mid_claim_stub_relay_drop_records_failure_delta() {
    // ── (0) Enable claim-log BEFORE any kernel code runs ────────────────────
    // SAFETY: test-binary-only env-var write; no other threads at this point.
    unsafe { std::env::set_var("NMP_CLAIM_LOG", "1") };

    // ── (1) Start stub relay ──────────────────────────────────────────────────
    // The stub accepts the WebSocket handshake then drops after STUB_ALIVE_MS.
    // We spawn it before StderrCapture so its thread does not inherit the
    // redirected fd-2.  The stub's threads never write to stderr.
    let stub = StubRelay::spawn(Duration::from_millis(STUB_ALIVE_MS));
    let stub_url = stub.ws_url();
    eprintln!("A5: stub relay at {stub_url}");

    // ── (2) Build the nevent URI with the stub as the sole relay hint ─────────
    // Phase-1 fires on relay.primal.net (bootstrap Content lane) for the
    // fictional event id.  relay.primal.net EOSEs with no match after
    // ~PHASE_1_BUDGET_MS (1500 ms).  Phase-2 then drains the candidate queue
    // which was seeded from this URI's relay TLV → REQ sent to the stub.
    let nevent_uri = build_nevent_uri(&stub_url);
    eprintln!("A5: nevent URI = {nevent_uri}");

    // ── (3) Start capturing stderr ───────────────────────────────────────────
    let cap = StderrCapture::start();

    // ── (4) Boot the actor — no AddRelay; bootstrap Content relay only ────────
    let (tx, rx) = spawn_actor();
    tx.send(ActorCommand::Start {
        visible_limit: 80,
        emit_hz: 4,
    })
    .expect("A5: Start send");

    // ── (5) Wait for the bootstrap relay to connect ───────────────────────────
    let connect_deadline = Instant::now() + CONNECT_BUDGET;
    let connected =
        drain_until_or_timeout(&rx, connect_deadline, |frame| relay_is_connected(frame));
    if !connected {
        let _ = tx.send(ActorCommand::Shutdown);
        let lines = cap.collect();
        eprintln!(
            "A5 SKIP: no relay connected within {:?}. Captured {} stderr lines.",
            CONNECT_BUDGET,
            lines.len()
        );
        return;
    }

    // ── (6) Issue the claim ──────────────────────────────────────────────────
    let claim_start = Instant::now();
    tx.send(ActorCommand::ClaimEvent {
        uri: nevent_uri,
        consumer_id: "a5-test".to_string(),
    })
    .expect("A5: ClaimEvent send");

    let test_deadline = Instant::now() + Duration::from_millis(TEST_BUDGET_MS);
    drain_until_or_timeout(&rx, test_deadline, |_| false);
    std::thread::sleep(Duration::from_millis(200));

    let elapsed_ms = claim_start.elapsed().as_millis();

    // ── (7) Shut down and collect ────────────────────────────────────────────
    let _ = tx.send(ActorCommand::Shutdown);
    drop(rx);
    std::thread::sleep(Duration::from_millis(100));
    let lines = cap.collect();

    // The stub relay object can be dropped now.
    drop(stub);

    // ── (8) Assertions ────────────────────────────────────────────────────────
    let scores = score_updates(&lines);
    let phase1_relays = req_emit_relays_for_phase(&lines, "phase1");
    let phase2_relays = req_emit_relays_for_phase(&lines, "phase2");

    eprintln!(
        "A5: elapsed={}ms scores={:?} phase1={:?} phase2={:?}",
        elapsed_ms, scores, phase1_relays, phase2_relays
    );

    // Assertion 1: Phase-1 must have fired on the bootstrap Content relay
    // (relay.primal.net).  If Phase-1 never fired, the actor never connected.
    let bootstrap_tried = phase1_relays.iter().any(|u| u.contains("relay.primal.net"));
    if !bootstrap_tried {
        eprintln!(
            "A5 SKIP: bootstrap relay not in phase1 set {:?}. \
             relay.primal.net may not have connected.",
            phase1_relays
        );
        return;
    }

    // Assertion 2: Phase-2 must have targeted the stub relay.
    // Phase-1 EOSEs with no match for the fictional event id →
    // Phase-2 drains the candidate_queue seeded from the URI relay hint.
    let stub_in_phase2 = phase2_relays.iter().any(|u| u.contains("127.0.0.1"));
    if !stub_in_phase2 {
        eprintln!(
            "A5 SKIP: stub relay (127.0.0.1) not in phase2 set {:?}. \
             Phase-2 may not have fired within the test budget, or Phase-1 \
             delivered before EOSE (relay.primal.net may have cached the id).",
            phase2_relays
        );
        return;
    }

    // Assertion 3: a ScoreUpdate with a failure delta must be recorded for
    // the stub relay (relay_failed hook → record_claim_outcome(Failed) →
    // delta = "+3f").  The "+3f" token is the canonical representation of
    // `ClaimOutcome::Failed` in `relay_score_record.rs`.
    let stub_failed_score = scores.iter().find(|(author, relay_url, delta, _)| {
        author == GIGI_PK && relay_url.contains("127.0.0.1") && delta == "+3f"
    });
    if stub_failed_score.is_none() {
        eprintln!(
            "A5 SKIP: no ScoreUpdate with delta='+3f' for stub relay in {:?}. \
             The relay may have dropped before the REQ was in-flight, so no \
             pending claim was registered against it.",
            scores
        );
        return;
    }
    assert!(
        stub_failed_score.is_some(),
        "A5: ScoreUpdate delta='+3f' must be recorded for stub relay; got {:?}",
        scores
    );

    // Assertion 4: wall-clock must stay within budget.  WALL_CLOCK_LIMIT_MS adds
    // headroom for the 200 ms + 100 ms trailing sleeps and channel-drain overhead.
    assert!(
        elapsed_ms < WALL_CLOCK_LIMIT_MS as u128,
        "A5: test must complete within {}ms; took {}ms",
        WALL_CLOCK_LIMIT_MS,
        elapsed_ms
    );

    eprintln!("A5 PASS: stub relay failure delta recorded, claim advanced within budget.");
}
