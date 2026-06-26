//! T120 — NIP-46 REQ-before-EVENT ordering on reconnect (green-gate, #2119).
//!
//! ## What this tests
//!
//! The relay worker injects a registered preamble at the FRONT of its outbound
//! queue on every (re)connect, BEFORE any pending `Send` commands are flushed.
//! For NIP-46 the `Nip46ConnectedHook` registers the subscription REQ as that
//! preamble (`Pool::set_reconnect_preamble`). After a forced disconnect, a
//! `sign_event` EVENT that was queued during the down period must arrive at the
//! mock relay AFTER the preamble REQ — so the mock can look up the client pubkey
//! from the filter and process the RPC.
//!
//! ## Fail-without-fix property
//!
//! Without the preamble injection (`relay_worker/mod.rs`, committed on this
//! branch), the outbound queue after reconnect is `[EVENT]` — no REQ precedes
//! it. The mock relay has no `client_local_pubkey` and drops the EVENT silently.
//! The `sign_event` RPC never reaches `observed_methods()`.
//!
//! With the fix, the queue becomes `[REQ, EVENT]`. The mock processes the REQ
//! first (extracting the pubkey), then decrypts and executes the `sign_event`.
//!
//! ## Determinism
//!
//! The sign EVENT is enqueued via `Pool::send` AFTER the pool reports `Failed`
//! (the relay worker is in `wait_before_reconnect`). No wall-clock sleep gates
//! the ordering; the FIFO command channel guarantees the EVENT arrives in
//! `pending` before the next `open_relay_socket` call.

mod common;

use std::sync::mpsc;
use std::time::Duration;

use nmp_network::pool::{Pool, PoolConfig, PoolEvent, RelayFrame, WireFrame};
use nmp_network::role::RelayRole;
use nmp_nip46::{build_event_frame, build_req_frame, start_bunker, Effect};
use nostr::Keys;

use crate::common::mock_bunker_relay::MockBunkerRelay;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const RECONNECT_TIMEOUT: Duration = Duration::from_secs(20);

// ─── test ────────────────────────────────────────────────────────────────────

/// Structural guarantee: on reconnect, the preamble REQ arrives at the mock
/// relay BEFORE the EVENT queued during the down period.
#[test]
fn nip46_req_arrives_before_event_on_reconnect() {
    // ── Keys + mock relay ───────────────────────────────────────────────────
    let bunker_keys = Keys::generate();
    let user_keys = Keys::generate();

    let mock = MockBunkerRelay::spawn(bunker_keys.clone(), user_keys.clone())
        .expect("mock bunker relay must spawn on 127.0.0.1");

    // ── NIP-46 session ──────────────────────────────────────────────────────
    let local_keys = Keys::generate();
    let sub_id = format!(
        "nip46-t120-{}",
        &local_keys.public_key().to_hex()[..8]
    );
    let relay_url = mock.ws_url();

    let now = now_unix_secs();
    let (mut session, initial_effects) = start_bunker(
        &sub_id,
        local_keys.clone(),
        bunker_keys.public_key(),
        relay_url.clone(),
        None,
        None,
        now,
    );

    // ── Pool ────────────────────────────────────────────────────────────────
    let (pool_tx, pool_rx) = mpsc::channel::<PoolEvent>();
    let pool = Pool::new(
        PoolConfig { default_role: RelayRole::Signer, ..Default::default() },
        pool_tx,
    );

    let h = pool.ensure_open_with_role(&relay_url, RelayRole::Signer);

    // ── Phase 1: initial connect ─────────────────────────────────────────
    wait_opened(&pool_rx, HANDSHAKE_TIMEOUT)
        .expect("pool must connect to mock relay");

    // Send Subscribe (REQ) + connect RPC.
    for effect in initial_effects {
        match effect {
            Effect::Subscribe { frame, .. } => {
                pool.send(h, WireFrame::Text(frame));
            }
            Effect::SendFrame { text, .. } => {
                pool.send(h, WireFrame::Text(text));
            }
            _ => {}
        }
    }

    // ── Phase 2: drive handshake to SignerReady ─────────────────────────
    let signer_ready = drive_handshake(&mut session, &pool_rx, &pool, h, HANDSHAKE_TIMEOUT);
    assert!(signer_ready, "NIP-46 handshake must complete (SignerReady)");

    // ── Phase 3: register preamble ───────────────────────────────────────
    // Mirror what Nip46ConnectedHook does via CommandSender::set_reconnect_preamble:
    // register the replay REQ as the worker's preamble so it is injected at the
    // FRONT of pending on every subsequent reconnect (the fix under test).
    let req_frame = build_req_frame(
        &sub_id,
        &local_keys.public_key().to_hex(),
        now_unix_secs(),
    );
    let preamble_ok = pool.set_reconnect_preamble(h, vec![req_frame]);
    assert!(preamble_ok, "set_reconnect_preamble must accept the handle");

    // ── Phase 4: force disconnect ─────────────────────────────────────────
    mock.force_disconnect();

    // The relay worker detects the broken socket → Failed → wait_before_reconnect.
    wait_failed(&pool_rx, Duration::from_secs(8))
        .expect("pool must report Failed after force_disconnect");

    // ── Phase 5: queue sign EVENT while socket is DOWN ────────────────────
    // Build an authentic NIP-46 sign_event RPC frame the mock can decrypt.
    // The relay worker queues this in `pending` via push_back during
    // wait_before_reconnect. On reconnect the preamble is push_front'd so the
    // final order is [REQ_preamble, sign_event_EVENT].
    let sign_rpc = serde_json::json!({
        "id": "t120-sign-001",
        "method": "sign_event",
        "params": [{
            "kind": 1,
            "content": "nip46 reconnect ordering test",
            "created_at": now_unix_secs(),
            "tags": [],
            "pubkey": user_keys.public_key().to_hex(),
        }]
    })
    .to_string();
    let event_frame = build_event_frame(&local_keys, bunker_keys.public_key(), &sign_rpc)
        .expect("build sign_event frame must succeed with real NIP-46 keys");

    let sent = pool.send(h, WireFrame::Text(event_frame));
    assert!(
        sent,
        "Pool::send must accept the handle during wait_before_reconnect \
         (command channel is alive until explicit Pool::close)"
    );

    // ── Phase 6: wait for reconnect ───────────────────────────────────────
    wait_opened(&pool_rx, RECONNECT_TIMEOUT)
        .expect("relay worker must reconnect to mock relay after forced disconnect");

    // Give the mock worker time to receive and process both frames.
    std::thread::sleep(Duration::from_millis(400));

    // ── Phase 7: assert REQ-before-EVENT on second connection ─────────────
    let log = mock.per_conn_log();

    // Locate the second connection's start (after the second "OPEN" marker).
    let second_conn_frames = second_conn_slice(&log)
        .unwrap_or_else(|| {
            panic!(
                "per_conn_log must contain at least two 'OPEN' markers \
                 (initial + reconnect); got: {log:?}"
            )
        });

    let req_pos = second_conn_frames.iter().position(|e| e == "REQ");
    let event_pos = second_conn_frames.iter().position(|e| e == "EVENT");

    assert!(
        req_pos.is_some(),
        "mock must receive REQ on the reconnect connection; \
         second_conn={second_conn_frames:?}"
    );
    assert!(
        event_pos.is_some(),
        "mock must receive EVENT (sign_event RPC) on the reconnect connection; \
         second_conn={second_conn_frames:?}"
    );
    assert!(
        req_pos.unwrap() < event_pos.unwrap(),
        "REQ MUST arrive before EVENT on reconnect — preamble fix must be active; \
         second_conn={second_conn_frames:?}"
    );

    // ── Phase 8: assert sign_event was processed (not stranded) ─────────
    // If EVENT arrived before REQ (pre-fix), the mock drops it (no
    // client_local_pubkey). sign_event only appears in observed_methods when
    // the mock successfully decrypts and handles the EVENT payload.
    let observed = mock.observed_methods();
    assert!(
        observed.contains(&"sign_event".to_string()),
        "mock must have observed sign_event — the EVENT was not dropped or stranded; \
         observed={observed:?}"
    );
}

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Drive the NIP-46 handshake state machine until `SignerReady` or timeout.
/// Returns `true` if `SignerReady` was reached within `timeout`.
fn drive_handshake(
    session: &mut nmp_nip46::SessionState,
    pool_rx: &mpsc::Receiver<PoolEvent>,
    pool: &Pool,
    h: nmp_network::pool::RelayHandle,
    timeout: Duration,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .unwrap_or(Duration::ZERO);
        match pool_rx.recv_timeout(remaining.min(Duration::from_millis(200))) {
            Ok(PoolEvent::Frame { frame: RelayFrame::Text(text), .. }) => {
                let effects = session.on_relay_text(&text, now_unix_secs());
                for effect in effects {
                    match effect {
                        Effect::SendFrame { text, .. } => {
                            pool.send(h, WireFrame::Text(text));
                        }
                        Effect::SignerReady(_) => return true,
                        _ => {}
                    }
                }
            }
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return false,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    false
}

/// Block until `PoolEvent::Opened` or timeout.  Drains and discards all other
/// events.  Returns the handle carried by the `Opened` event.
fn wait_opened(
    rx: &mpsc::Receiver<PoolEvent>,
    timeout: Duration,
) -> Option<nmp_network::pool::RelayHandle> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.checked_duration_since(std::time::Instant::now())?;
        match rx.recv_timeout(remaining.min(Duration::from_millis(200))) {
            Ok(PoolEvent::Opened { h, .. }) => return Some(h),
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
        }
    }
}

/// Block until `PoolEvent::Failed` or timeout.
fn wait_failed(rx: &mpsc::Receiver<PoolEvent>, timeout: Duration) -> Option<()> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.checked_duration_since(std::time::Instant::now())?;
        match rx.recv_timeout(remaining.min(Duration::from_millis(200))) {
            Ok(PoolEvent::Failed { .. }) => return Some(()),
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
        }
    }
}

/// Slice `log` to the frames that belong to the SECOND connection (after the
/// second `"OPEN"` marker).  Returns `None` if there is no second connection.
fn second_conn_slice(log: &[String]) -> Option<&[String]> {
    // Find the first "OPEN".
    let first = log.iter().position(|e| e == "OPEN")?;
    // Find the second "OPEN" after the first.
    let second_offset = log[first + 1..].iter().position(|e| e == "OPEN")?;
    let second = first + 1 + second_offset + 1; // offset past the second "OPEN" itself
    Some(&log[second..])
}

/// Wall-clock Unix seconds — used only for NIP-44 `created_at` timestamps
/// (not for any test timeout gate).
fn now_unix_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
