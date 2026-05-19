//! Shared harness used by the four `nip77_*` integration tests.
//!
//! Provides an in-process pairing of a client and server [`Reconciler`]
//! plus an "ID-pull" helper that returns the bytes the negentropy session
//! exchanged and the ids the client ended up needing.  No WebSocket or
//! tungstenite — the reconciler is transport-agnostic by design, so the
//! tests exercise the protocol contract directly.
//!
//! Files using this module include `mod nip77_common;` to pull it in.

#![allow(dead_code)]

use nmp_nip77::{Reconciler, ReconcilerOutcome, SyncedItem};

/// One pairing round.
pub struct ReconcileSession {
    pub bytes_client_to_server: u64,
    pub bytes_server_to_client: u64,
    pub need: Vec<[u8; 32]>,
    pub have: Vec<[u8; 32]>,
    pub resume_state: Vec<u8>,
}

/// Drive a full client↔server reconciliation to completion.  Panics on any
/// unexpected protocol error or if the loop fails to converge in
/// `max_rounds` rounds (32 is plenty for sets up to ~50k items).
pub fn reconcile_in_process(
    client_items: Vec<SyncedItem>,
    server_items: Vec<SyncedItem>,
    max_rounds: usize,
) -> ReconcileSession {
    let mut client = Reconciler::client(client_items).expect("build client");
    let mut server = Reconciler::server(server_items).expect("build server");

    let mut peer_to_client: Option<Vec<u8>> = None;
    let mut bytes_c2s: u64 = 0;
    let mut bytes_s2c: u64 = 0;
    for _ in 0..max_rounds {
        let outcome = client.step(peer_to_client.as_deref()).expect("client step");
        let bytes = match outcome {
            ReconcilerOutcome::Send(b) => b,
            ReconcilerOutcome::Done {
                have,
                need,
                state,
            } => {
                return ReconcileSession {
                    bytes_client_to_server: bytes_c2s,
                    bytes_server_to_client: bytes_s2c,
                    need,
                    have,
                    resume_state: state,
                };
            }
        };
        bytes_c2s += bytes.len() as u64;
        let response = server.step(Some(&bytes)).expect("server step");
        let server_bytes = match response {
            ReconcilerOutcome::Send(b) => b,
            ReconcilerOutcome::Done { .. } => Vec::new(),
        };
        bytes_s2c += server_bytes.len() as u64;
        peer_to_client = Some(server_bytes);
    }
    panic!("reconciliation did not converge in {max_rounds} rounds");
}

/// Synthesise `count` items with monotonically increasing timestamps and
/// deterministic ids.  The id derives from the *absolute* `base_ts + i`
/// pair, not from `i` alone — that way two callers using different `base_ts`
/// produce non-overlapping id sets, which is exactly what the
/// reconnect-resumes-from-watermark gate needs to simulate a real-world gap.
pub fn synth_items(count: u32, base_ts: u64) -> Vec<SyncedItem> {
    (0..count)
        .map(|i| {
            let ts = base_ts + i as u64;
            let mut id = [0u8; 32];
            // Pack the timestamp into the id so distinct `(base_ts, i)` pairs
            // always yield distinct ids.  Use big-endian so visual debugging
            // is straightforward in test failure output.
            id[24..].copy_from_slice(&ts.to_be_bytes());
            SyncedItem { created_at: ts, id }
        })
        .collect()
}

/// Worst-case REQ wire cost for a `count`-event backfill, assuming each
/// event renders to ~`avg_bytes` bytes of `["EVENT", subid, {...}]` JSON.
/// Used as the baseline for "bytes-on-wire saved" assertions.
pub fn req_baseline_bytes(count: u32, avg_bytes: u64) -> u64 {
    count as u64 * avg_bytes
}
