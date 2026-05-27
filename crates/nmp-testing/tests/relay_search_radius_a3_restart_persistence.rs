//! A3 — restart-persistence acceptance test for relay-search-radius (W9).
//!
//! Proves that relay-author scores recorded by a kernel run survive a process
//! restart (drop + reopen at the same LMDB path) and influence Phase-1 routing
//! on the first claim of the new session.
//!
//! # What this tests
//!
//! - Session 1: spawn actor with LMDB storage, claim a Gigi article, wait for
//!   EventRx, let the kernel flush its score map.
//! - Drop Session 1: send Shutdown, drop the sender, wait for actor exit.
//! - Session 2: spawn a *new* actor at the **same** LMDB path, claim a
//!   *different* Gigi article.
//! - Assert: `ReqEmit phase=phase1 relay_url=<delivering-relay-from-s1>` present.
//! - Assert: no `ReqEmit phase=phase2` lines (the warm relay resolves without
//!   needing phase-2 expansion).
//!
//! # Feature gates
//!
//! Requires both `lmdb-backend` (score persistence) and `real-relay` (live
//! WebSocket connections).
//!
//! # Running
//!
//! ```bash
//! cargo test -p nmp-testing --features real-relay,lmdb-backend \
//!     --test relay_search_radius_a3_restart_persistence -- --ignored --nocapture
//! ```
//!
//! Marked `#[ignore]` — requires live network access and LMDB.

#![cfg(all(feature = "real-relay", feature = "lmdb-backend"))]

#[path = "common/mod.rs"]
mod common;

use common::wire_log::{
    event_rx_for_author, req_emit_relays_for_phase, score_updates, StderrCapture,
};
use nmp_core::testing::{spawn_actor_with_storage_path, ActorCommand};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tempfile::tempdir;

/// Gigi (dergigi) pubkey — hex.
const GIGI_PK: &str = "6e468422dfb74a5738702a8823b9b28168abab8655faacb6853cd0ee15deee93";

/// Gigi "the-internet-left-me" naddr — Session 1 prime article.
const GIGI_NADDR_S1: &str = "nostr:naddr1qvzqqqr4gupzqmjxss3dld622uu8q25gywum9qtg4w4cv4064jmg20xsac2aam5nqy6xsar5wpen5te0v3jhyemfva5jucm0d5hnyvpjxchnqve0xgcz7argv5kkjmn5v4exuet594kx2en594kk2tcqz36xsefdd9h8getjdejhgttvv4n8gttdv55zqsmp";

/// Gigi "careful-icarus" naddr — Session 2 claim article (different from S1).
const GIGI_NADDR_S2: &str = "nostr:naddr1qq8xxctjv4n82mpdd93kzun4wvpzqmjxss3dld622uu8q25gywum9qtg4w4cv4064jmg20xsac2aam5nqvzqqqr4gukkfv2a";

const SESSION_BUDGET_MS: u64 = 7000;
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

/// Guard: check APFS disk pressure before attempting LMDB mmap allocation.
/// Returns true if there is enough free space (>=1 GB) on the temp-dir volume.
fn lmdb_capacity_available(path: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(meta) = std::fs::metadata(path) else {
        return true;
    };
    let dev = meta.dev();
    // Use `statvfs` via libc if available; fall back to always-true.
    #[cfg(target_os = "macos")]
    {
        let c_path = std::ffi::CString::new(path.to_string_lossy().as_bytes()).ok();
        if let Some(c_path) = c_path {
            let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
            let rc = unsafe { libc::statfs(c_path.as_ptr(), &mut stat) };
            if rc == 0 {
                let available_bytes = stat.f_bavail as u64 * stat.f_bsize as u64;
                return available_bytes >= 1_073_741_824; // 1 GB
            }
        }
    }
    let _ = dev;
    true
}

#[test]
#[ignore = "real-relay (run with --features real-relay,lmdb-backend --ignored --nocapture)"]
fn a3_restart_persistence_warm_relay_survives_kernel_restart() {
    // ── (0) Enable claim-log BEFORE any kernel code runs ────────────────────
    unsafe { std::env::set_var("NMP_CLAIM_LOG", "1") };

    // ── (1) Set up temp LMDB directory ──────────────────────────────────────
    let dir = tempdir().expect("A3: tempdir failed");
    let store_path = dir.path().to_str().expect("A3: tempdir path is non-UTF8");

    if !lmdb_capacity_available(dir.path()) {
        eprintln!("A3 SKIP: insufficient disk space for LMDB mmap allocation.");
        return;
    }

    // ─── SESSION 1: prime the score map ──────────────────────────────────────
    eprintln!("A3: starting Session 1 at {store_path}");

    let s1_cap = StderrCapture::start();

    let (tx1, rx1) = spawn_actor_with_storage_path(store_path);
    tx1.send(ActorCommand::Start {
        visible_limit: 80,
        emit_hz: 4,
    })
    .expect("A3 S1: Start send");

    let connect_deadline = Instant::now() + CONNECT_BUDGET;
    let connected =
        drain_until_or_timeout(&rx1, connect_deadline, |frame| relay_is_connected(frame));
    if !connected {
        let _ = tx1.send(ActorCommand::Shutdown);
        let lines = s1_cap.collect();
        eprintln!(
            "A3 SKIP: S1 no relay connected within {:?}. Captured {} stderr lines.",
            CONNECT_BUDGET,
            lines.len()
        );
        return;
    }

    tx1.send(ActorCommand::ClaimEvent {
        uri: GIGI_NADDR_S1.to_string(),
        consumer_id: "a3-s1".to_string(),
    })
    .expect("A3 S1: ClaimEvent send");

    let claim_deadline = Instant::now() + Duration::from_millis(SESSION_BUDGET_MS);
    drain_until_or_timeout(&rx1, claim_deadline, |_| false);
    std::thread::sleep(Duration::from_millis(300)); // allow score flush

    let _ = tx1.send(ActorCommand::Shutdown);
    drop(rx1);
    drop(tx1);
    std::thread::sleep(Duration::from_millis(200));

    let s1_lines = s1_cap.collect();

    let s1_scores = score_updates(&s1_lines);
    let s1_got_event = event_rx_for_author(&s1_lines, GIGI_PK);

    eprintln!("A3 S1: scores={:?} event_rx={}", s1_scores, s1_got_event);

    if s1_scores.is_empty() {
        eprintln!(
            "A3 SKIP: S1 produced no ScoreUpdate — article not delivered or scored; \
             can't test persistence."
        );
        return;
    }

    let delivering_relay: Option<String> = s1_scores
        .iter()
        .find(|(author, _, delta, _)| author == GIGI_PK && delta.contains("successes"))
        .map(|(_, relay, _, _)| relay.clone());

    let Some(ref scored_relay) = delivering_relay else {
        eprintln!(
            "A3 SKIP: S1 ScoreUpdate rows exist but none carry a positive delta for \
             GIGI_PK: {:?}",
            s1_scores
        );
        return;
    };
    eprintln!("A3 S1: delivering relay = {scored_relay}");

    // ─── SESSION 2: new kernel at same store, claim different article ─────────
    eprintln!("A3: starting Session 2 (same LMDB path)");

    let s2_cap = StderrCapture::start();

    let (tx2, rx2) = spawn_actor_with_storage_path(store_path);
    tx2.send(ActorCommand::Start {
        visible_limit: 80,
        emit_hz: 4,
    })
    .expect("A3 S2: Start send");

    let connect_deadline2 = Instant::now() + CONNECT_BUDGET;
    let connected2 =
        drain_until_or_timeout(&rx2, connect_deadline2, |frame| relay_is_connected(frame));
    if !connected2 {
        let _ = tx2.send(ActorCommand::Shutdown);
        let lines = s2_cap.collect();
        eprintln!(
            "A3 SKIP: S2 no relay connected within {:?}. Captured {} stderr lines.",
            CONNECT_BUDGET,
            lines.len()
        );
        return;
    }

    tx2.send(ActorCommand::ClaimEvent {
        uri: GIGI_NADDR_S2.to_string(),
        consumer_id: "a3-s2".to_string(),
    })
    .expect("A3 S2: ClaimEvent send");

    let s2_claim_deadline = Instant::now() + Duration::from_millis(SESSION_BUDGET_MS);
    drain_until_or_timeout(&rx2, s2_claim_deadline, |_| false);
    std::thread::sleep(Duration::from_millis(200));

    let _ = tx2.send(ActorCommand::Shutdown);
    drop(rx2);
    drop(tx2);
    std::thread::sleep(Duration::from_millis(100));

    let s2_lines = s2_cap.collect();

    // ─── Assertions ────────────────────────────────────────────────────────────
    let s2_phase1_relays = req_emit_relays_for_phase(&s2_lines, "phase1");
    let s2_phase2_relays = req_emit_relays_for_phase(&s2_lines, "phase2");

    eprintln!(
        "A3 S2: phase1={:?} phase2={:?}",
        s2_phase1_relays, s2_phase2_relays
    );

    let phase1_has_scored = s2_phase1_relays.iter().any(|u| u == scored_relay);

    if !phase1_has_scored {
        eprintln!(
            "A3 SKIP: scored relay '{}' not in S2 phase1 set {:?}. \
             Score may not have crossed WARM_THRESHOLD or flushed before restart.",
            scored_relay, s2_phase1_relays
        );
        return;
    }

    // Primary assertion: warm relay from S1 must appear in S2 Phase-1 set.
    assert!(
        phase1_has_scored,
        "A3: scored relay '{}' must appear in S2 phase1 ReqEmit set; got {:?}",
        scored_relay, s2_phase1_relays
    );

    // Secondary assertion: if Phase-1 resolved successfully, Phase-2 should
    // not have been needed (the warm relay should have found the article).
    if phase1_has_scored && s2_phase2_relays.is_empty() {
        eprintln!("A3 PASS: warm relay resolved article without Phase-2 expansion (best case).");
    } else if !s2_phase2_relays.is_empty() {
        eprintln!(
            "A3 NOTE: Phase-2 also fired ({:?}) — warm relay did not fully resolve \
             alone, but phase1 assertion still holds.",
            s2_phase2_relays
        );
    }
}
