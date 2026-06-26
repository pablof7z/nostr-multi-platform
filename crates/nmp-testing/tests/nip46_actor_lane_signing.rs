//! T121 — NIP-46 actor-lane sign-ready end-to-end (PR-B1, #2119).
//!
//! ## What this tests
//!
//! The `nmp-nip46-runtime` actor-lane path is fully sign-ready:
//!
//! 1. `start_bunker` creates the session; initial effects drive the Pool.
//! 2. The NIP-46 handshake (`connect` + `get_public_key`) runs to completion
//!    against a real [`MockBunkerRelay`] WebSocket.
//! 3. On [`Effect::SignerReady`], a real `Nip46Signer` is built using
//!    [`Nip46SignerHandle::complete`] with an [`ActorLaneTransport`].
//! 4. `signer.sign(unsigned_event)` enqueues a `sign_event` RPC via
//!    `ActorLaneTransport::send_rpc`, which posts `EnqueueOutbound` to the
//!    actor inbox channel.
//! 5. A pump thread drains `EnqueueOutbound` commands from the actor channel
//!    and routes them to the Pool (actor → relay wire).
//! 6. The pump thread also drains inbound Pool frames, calls
//!    `decode_inbound_response` (the steady-state decode path), and delivers
//!    the decoded body via `Nip46Signer::ingest_rpc_response` so the parking
//!    map resolves the pending sign operation.
//! 7. `sign_op.wait(timeout)` returns a [`SignedEvent`] validated by
//!    `nostr::Event::verify()` inside the mapper.
//!
//! ## What is NOT tested here
//!
//! - The full actor loop (interceptor + kernel). That requires the irreversible
//!   flip in PR-B2. This test proves the runtime components are sign-ready
//!   in isolation.
//! - Multi-relay fan-out (covered by unit tests in transport_tests.rs).
//! - Restore / `init_restore` path (covered by unit tests).
//!
//! ## Fail-without-fix property
//!
//! Without the PR-B1 `Nip46Signer` + `decode_inbound_response` wiring,
//! `sign_op.wait()` hangs until timeout and the test fails with:
//! > "sign must succeed: Err(Backend("nip46-runtime PR-A: sign response parking not yet wired"))"
//!
//! With the fix, the sign round-trip completes in well under 1 second against
//! the local mock relay.

mod common;

use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use nmp_core::actor::ActorCommand;
use nmp_core::{ActorMail, CommandSender};
use nmp_network::pool::{Pool, PoolConfig, PoolEvent, RelayFrame, WireFrame};
use nmp_network::role::RelayRole;
use nmp_nip46::{decode_inbound_response, start_bunker, Effect, SignerReady};
use nmp_nip46_runtime::transport::ActorLaneTransport;
use nmp_signers::{Nip46Signer, Nip46SignerHandle};
use nmp_signer_iface::{RemoteSignerHandle, UnsignedEvent};
use nostr::{Keys, PublicKey};

use crate::common::mock_bunker_relay::MockBunkerRelay;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const SIGN_TIMEOUT: Duration = Duration::from_secs(10);

// ─── test ────────────────────────────────────────────────────────────────────

/// Full actor-lane sign round-trip: handshake → Nip46Signer built →
/// sign_event routed via ActorLaneTransport → decode_inbound_response →
/// ingest_rpc_response → Schnorr-valid SignedEvent.
#[test]
fn actor_lane_sign_round_trip() {
    // ── Keys + mock relay ────────────────────────────────────────────────────
    let bunker_keys = Keys::generate();
    let user_keys = Keys::generate();
    let mock = MockBunkerRelay::spawn(bunker_keys.clone(), user_keys.clone())
        .expect("mock bunker relay must spawn on 127.0.0.1");

    // ── NIP-46 session init ──────────────────────────────────────────────────
    let local_keys = Keys::generate();
    let sub_id = format!("nip46-t121-{}", &local_keys.public_key().to_hex()[..8]);
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

    // ── Pool + CommandSender spy ─────────────────────────────────────────────
    let (pool_tx, pool_rx) = mpsc::channel::<PoolEvent>();
    let pool = Arc::new(Pool::new(
        PoolConfig { default_role: RelayRole::Signer, ..Default::default() },
        pool_tx,
    ));

    // Actor inbox spy: captures EnqueueOutbound from ActorLaneTransport::send_rpc.
    let (actor_tx, actor_rx) = mpsc::channel::<ActorMail>();
    let sender = CommandSender::new(actor_tx);

    let h = pool.ensure_open_with_role(&relay_url, RelayRole::Signer);

    // Wait for the Pool to open the WebSocket connection to MockBunkerRelay.
    wait_opened(&pool_rx, HANDSHAKE_TIMEOUT).expect("pool must connect to mock relay");

    // ── Send initial effects (REQ + connect RPC) ─────────────────────────────
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

    // ── Drive handshake to SignerReady ────────────────────────────────────────
    let ready = drive_handshake_to_signer_ready(
        &mut session,
        &pool_rx,
        &pool,
        h,
        HANDSHAKE_TIMEOUT,
    )
    .expect("NIP-46 handshake must reach SignerReady within timeout");

    // ── Build Nip46Signer after handshake ─────────────────────────────────────
    let remote_signer_pubkey = PublicKey::from_hex(&ready.remote_signer_pubkey_hex)
        .expect("remote_signer_pubkey_hex must be valid hex");
    let user_pubkey = PublicKey::from_hex(&ready.user_pubkey_hex)
        .expect("user_pubkey_hex must be valid hex");

    // Build the actor-lane transport — fans outbound RPCs to the actor inbox.
    let transport = ActorLaneTransport::new_multi(
        sender.clone(),
        local_keys.clone(),
        remote_signer_pubkey,
        vec![relay_url.clone()],
    );

    // Reconstruct the bunker URI with the known remote signer pubkey + relay.
    // This is the same synthetic-URI pattern the interceptor uses in
    // `handle_signer_ready` (see interceptor.rs).
    let relay_param = nmp_nip46::percent_encode_query_value(&relay_url);
    let synthetic_uri =
        format!("bunker://{}?relay={}", remote_signer_pubkey.to_hex(), relay_param);
    let signer_handle = Nip46SignerHandle::from_bunker_uri_with_local_key(
        &synthetic_uri,
        local_keys.secret_key().clone(),
    )
    .expect("synthetic bunker URI must parse");

    // complete() upgrades the pre-handshake handle to a fully-connected signer.
    let signer = Arc::new(signer_handle.complete(Arc::new(transport), user_pubkey));

    // ── Pump thread ───────────────────────────────────────────────────────────
    // Routes EnqueueOutbound commands from the actor inbox to the Pool (outbound
    // path: signer → mock relay) and decodes inbound relay frames and delivers
    // them to the signer's parking map (steady-state decode path).
    let signer_for_pump = Arc::clone(&signer);
    let pool_for_pump = Arc::clone(&pool);
    let local_keys_pump = local_keys.clone();
    // pool_rx is moved into the pump thread after the handshake (main thread no longer needs it).
    std::thread::spawn(move || {
        loop {
            // 1. Drain outbound commands → pool.send
            while let Ok(mail) = actor_rx.recv_timeout(Duration::from_millis(10)) {
                if let ActorMail::Command(ActorCommand::EnqueueOutbound { text, .. }) = mail {
                    pool_for_pump.send(h, WireFrame::Text(text));
                }
            }
            // 2. Drain inbound relay frames → decode → ingest_rpc_response
            while let Ok(event) = pool_rx.recv_timeout(Duration::from_millis(10)) {
                if let PoolEvent::Frame { frame: RelayFrame::Text(text), .. } = event {
                    // Parse ["EVENT", sub_id, {event}] — the mock relay wraps its reply as an EVENT.
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(arr) = v.as_array() {
                            if arr.first().and_then(|x| x.as_str()) == Some("EVENT") {
                                if let Some(event_obj) = arr.get(2) {
                                    // Steady-state decode: NIP-44 decrypt using
                                    // our local keys + bunker's signing pubkey.
                                    if let Some(body) = decode_inbound_response(
                                        event_obj,
                                        &local_keys_pump,
                                        remote_signer_pubkey,
                                    ) {
                                        // Deliver the decoded JSON-RPC body to the
                                        // signer's parking map. This resolves any
                                        // pending sign/encrypt op that sent the
                                        // matching request id.
                                        signer_for_pump.ingest_rpc_response(&body);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    // ── Sign an event ─────────────────────────────────────────────────────────
    let unsigned = UnsignedEvent {
        pubkey: user_pubkey.to_hex(),
        kind: 1,
        tags: vec![],
        content: "actor-lane PR-B1 sign round-trip test".to_string(),
        created_at: now_unix_secs(),
    };
    let sign_op = <Nip46Signer as RemoteSignerHandle>::sign(&signer, &unsigned);

    let signed = sign_op
        .wait(SIGN_TIMEOUT)
        .expect("sign must succeed within timeout — pump thread must route sign_event RPC");

    // ── Assertions ────────────────────────────────────────────────────────────
    // The mapper already ran `nostr::Event::verify()` before returning OK.
    // These assertions confirm identity (right pubkey, right content) and that
    // the mock relay actually processed the sign_event call.
    assert_eq!(
        signed.unsigned.pubkey,
        user_pubkey.to_hex(),
        "signed event pubkey must match user pubkey (NIP-46 sign_event cross-check)"
    );
    assert_eq!(
        signed.unsigned.content, unsigned.content,
        "signed event content must match what we asked to sign"
    );
    assert!(!signed.id.is_empty(), "signed event must have a non-empty id");
    assert!(!signed.sig.is_empty(), "signed event must have a non-empty schnorr signature");

    let observed = mock.observed_methods();
    assert!(
        observed.contains(&"connect".to_string()),
        "mock must have observed connect; got: {observed:?}"
    );
    assert!(
        observed.contains(&"get_public_key".to_string()),
        "mock must have observed get_public_key; got: {observed:?}"
    );
    assert!(
        observed.contains(&"sign_event".to_string()),
        "mock must have observed sign_event — sign_event RPC was routed through actor-lane; \
         got: {observed:?}"
    );
}

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Drive the NIP-46 handshake state machine until [`Effect::SignerReady`] or
/// timeout.  Returns the `SignerReady` payload on success.
fn drive_handshake_to_signer_ready(
    session: &mut nmp_nip46::SessionState,
    pool_rx: &mpsc::Receiver<PoolEvent>,
    pool: &Pool,
    h: nmp_network::pool::RelayHandle,
    timeout: Duration,
) -> Option<SignerReady> {
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
                        Effect::SignerReady(ready) => {
                            return Some(ready);
                        }
                        _ => {}
                    }
                }
            }
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    None
}

/// Block until `PoolEvent::Opened` or timeout.
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

/// Wall-clock Unix seconds — used only for NIP-44 `created_at` timestamps.
fn now_unix_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
