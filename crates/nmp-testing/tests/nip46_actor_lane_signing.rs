//! T121 — NIP-46 actor-lane sign-ready end-to-end (PR-B1, #2119).
//!
//! ## What this proves
//!
//! 1. The full `bunker://` handshake runs through the **runtime** state machine
//!    (`Nip46Runtime::on_relay_text`) against a real [`MockBunkerRelay`], not a
//!    standalone reducer — so the runtime reaches `Done` phase and its
//!    steady-state decode path is exercised.
//! 2. On `SignerReady`, [`record_signer_ready`] persists the learned remote
//!    signer pubkey (BLOCKER 1) and a real `Nip46Signer` is built with an
//!    [`ActorLaneTransport`].
//! 3. The sign RPC fans out via `EnqueueOutbound`; the pump routes it to the
//!    Pool; the mock signs and replies.
//! 4. The inbound response is decoded by `Nip46Runtime::on_relay_text` and
//!    delivered through the **real** seam:
//!    `CommandSender::deliver_signer_response` → an `ActorCommand::Identity(
//!    DeliverSignerResponse)` on the actor channel → drained → the registered
//!    signer's `deliver_response`.  The test never calls `ingest_rpc_response`
//!    directly — the parked op resolves only because the dispatch→deliver path
//!    works.
//!
//! Additional coverage:
//! - `init_bunker_fans_req_and_connect_to_all_relays` — BLOCKER 2 (multi-relay).
//! - `non_canonical_relay_uri_matches_inbound` — BLOCKER 3 (canonicalization).
//! - `nostrconnect_signer_ready_updates_decode_pubkey` — BLOCKER 1 (write-back).

mod common;

use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::Duration;

use nmp_core::actor::{ActorCommand, IdentityCommand};
use nmp_core::{canonical_relay_url, ActorMail, CommandSender};
use nmp_network::pool::{Pool, PoolConfig, PoolEvent, RelayFrame, RelayHandle, WireFrame};
use nmp_network::role::RelayRole;
use nmp_nip46::{build_event_frame, Effect, SignerReady};
use nmp_nip46_runtime::transport::ActorLaneTransport;
use nmp_nip46_runtime::{
    init_bunker, init_nostrconnect, new_nip46_runtime_handle, record_signer_ready,
    Nip46RuntimeHandle,
};
use nmp_signer_iface::{RemoteSignerHandle, UnsignedEvent};
use nmp_signers::{Nip46Signer, Nip46SignerHandle};
use nostr::{Keys, PublicKey};

use crate::common::mock_bunker_relay::MockBunkerRelay;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const SIGN_TIMEOUT: Duration = Duration::from_secs(10);

// ─── BLOCKER 4 — full real-path sign round-trip (bunker) ──────────────────────

/// Drive the bunker handshake through the runtime, then sign via the real
/// delivery seam (`deliver_signer_response` → `DeliverSignerResponse` → drain →
/// `deliver_response`).  No direct `ingest_rpc_response` call anywhere.
#[test]
fn actor_lane_sign_round_trip_real_delivery_path() {
    // ── Keys + mock relay ────────────────────────────────────────────────────
    let bunker_keys = Keys::generate();
    let user_keys = Keys::generate();
    let mock = MockBunkerRelay::spawn(bunker_keys.clone(), user_keys.clone())
        .expect("mock bunker relay must spawn on 127.0.0.1");

    // ── Runtime session init (bunker) ────────────────────────────────────────
    let local_keys = Keys::generate();
    let sub_id = format!("nip46-t121-{}", &local_keys.public_key().to_hex()[..8]);
    let relay_url = mock.ws_url();

    let handle = new_nip46_runtime_handle();
    let initial_effects = init_bunker(
        &handle,
        sub_id.clone(),
        local_keys.clone(),
        bunker_keys.public_key(),
        vec![relay_url.clone()],
        None,
        None,
        now_unix_secs(),
    )
    .expect("init_bunker must succeed");

    // ── Pool + CommandSender spy ─────────────────────────────────────────────
    let (pool_tx, pool_rx) = mpsc::channel::<PoolEvent>();
    let pool = Arc::new(Pool::new(
        PoolConfig {
            default_role: RelayRole::Signer,
            ..Default::default()
        },
        pool_tx,
    ));
    let (actor_tx, actor_rx) = mpsc::channel::<ActorMail>();
    let sender = CommandSender::new(actor_tx);

    let h = pool.ensure_open_with_role(&relay_url, RelayRole::Signer);
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

    // ── Drive handshake to SignerReady THROUGH the runtime ───────────────────
    let ready = drive_runtime_handshake(&handle, &pool_rx, &pool, h, &relay_url, HANDSHAKE_TIMEOUT)
        .expect("NIP-46 handshake must reach SignerReady within timeout");

    let remote_signer_pubkey = PublicKey::from_hex(&ready.remote_signer_pubkey_hex)
        .expect("remote_signer_pubkey_hex must be valid hex");
    let user_pubkey =
        PublicKey::from_hex(&ready.user_pubkey_hex).expect("user_pubkey_hex must be valid hex");

    // BLOCKER 1 seam: persist the learned remote signer pubkey (no-op for bunker
    // but exercises the exact call the interceptor makes on SignerReady).
    record_signer_ready(&handle, remote_signer_pubkey);

    // ── Build the real Nip46Signer (same as interceptor::handle_signer_ready) ─
    let transport = ActorLaneTransport::new_multi(
        sender.clone(),
        local_keys.clone(),
        remote_signer_pubkey,
        vec![relay_url.clone()],
    );
    let relay_param = nmp_nip46::percent_encode_query_value(&relay_url);
    let synthetic_uri = format!(
        "bunker://{}?relay={}",
        remote_signer_pubkey.to_hex(),
        relay_param
    );
    let signer_handle = Nip46SignerHandle::from_bunker_uri_with_local_key(
        &synthetic_uri,
        local_keys.secret_key().clone(),
    )
    .expect("synthetic bunker URI must parse");
    let signer = Arc::new(signer_handle.complete(Arc::new(transport), user_pubkey));

    // ── Pump thread: the REAL delivery path ──────────────────────────────────
    // Outbound: actor `EnqueueOutbound` → pool.send.
    // Delivery: `DeliverSignerResponse` (posted by deliver_signer_response) →
    //           signer.deliver_response (the dispatch fan-out the actor performs
    //           via deliver_to_remote_signers).  Inbound relay frames are decoded
    //           by the RUNTIME (`on_relay_text`) and routed through
    //           `deliver_signer_response` — NOT ingest_rpc_response.
    let signer_for_pump = Arc::clone(&signer);
    let pool_for_pump = Arc::clone(&pool);
    let handle_for_pump = Arc::clone(&handle);
    let sender_for_pump = sender.clone();
    let relay_for_pump = relay_url.clone();
    std::thread::spawn(move || loop {
        // 1. Drain actor commands.
        while let Ok(mail) = actor_rx.recv_timeout(Duration::from_millis(10)) {
            match mail {
                ActorMail::Command(ActorCommand::EnqueueOutbound { text, .. }) => {
                    pool_for_pump.send(h, WireFrame::Text(text));
                }
                ActorMail::Command(ActorCommand::Identity(
                    IdentityCommand::DeliverSignerResponse { response_json },
                )) => {
                    // This is the seam the actor's deliver_to_remote_signers runs.
                    signer_for_pump.deliver_response(&response_json);
                }
                _ => {}
            }
        }
        // 2. Drain inbound relay frames → runtime decode → deliver_signer_response.
        while let Ok(event) = pool_rx.recv_timeout(Duration::from_millis(10)) {
            if let PoolEvent::Frame {
                frame: RelayFrame::Text(text),
                ..
            } = event
            {
                let decoded = {
                    let mut guard = handle_for_pump.lock().expect("runtime lock");
                    guard
                        .as_mut()
                        .map(|rt| rt.on_relay_text(&relay_for_pump, &text, now_unix_secs()))
                };
                if let Some((_effects, Some(body))) = decoded {
                    sender_for_pump.deliver_signer_response(body);
                }
            }
        }
    });

    // ── Sign an event ─────────────────────────────────────────────────────────
    let unsigned = UnsignedEvent {
        pubkey: user_pubkey.to_hex(),
        kind: 1,
        tags: vec![],
        content: "actor-lane PR-B1 real-delivery sign round-trip".to_string(),
        created_at: now_unix_secs(),
    };
    let signed = <Nip46Signer as RemoteSignerHandle>::sign(&signer, &unsigned)
        .wait(SIGN_TIMEOUT)
        .expect(
            "sign must resolve via deliver_signer_response → DeliverSignerResponse → \
             deliver_response (NOT a direct ingest_rpc_response)",
        );

    // ── Assertions ────────────────────────────────────────────────────────────
    assert_eq!(signed.unsigned.pubkey, user_pubkey.to_hex());
    assert_eq!(signed.unsigned.content, unsigned.content);
    assert!(
        !signed.id.is_empty(),
        "signed event must have a non-empty id"
    );
    assert!(
        !signed.sig.is_empty(),
        "signed event must have a schnorr signature"
    );

    let observed = mock.observed_methods();
    for method in ["connect", "get_public_key", "sign_event"] {
        assert!(
            observed.contains(&method.to_string()),
            "mock must have observed {method}; got: {observed:?}"
        );
    }
}

// ─── BLOCKER 2 — multi-relay fan-out (REQ + connect to all relays) ────────────

/// `init_bunker` with multiple relays must fan BOTH the REQ (Subscribe) and the
/// connect EVENT (SendFrame) to EVERY relay — not just the primary.
#[test]
fn init_bunker_fans_req_and_connect_to_all_relays() {
    let local_keys = Keys::generate();
    let bunker_keys = Keys::generate();
    let sub_id = "nip46-multi".to_string();

    let relay_a = "wss://relay-a.example".to_string();
    let relay_b = "wss://relay-b.example".to_string();

    let handle = new_nip46_runtime_handle();
    let effects = init_bunker(
        &handle,
        sub_id,
        local_keys,
        bunker_keys.public_key(),
        vec![relay_a.clone(), relay_b.clone()],
        None,
        None,
        now_unix_secs(),
    )
    .expect("init_bunker must succeed");

    let subscribe_relays: Vec<&str> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::Subscribe { relay_url, .. } => Some(relay_url.as_str()),
            _ => None,
        })
        .collect();
    let sendframe_relays: Vec<&str> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::SendFrame { relay_url, .. } => Some(relay_url.as_str()),
            _ => None,
        })
        .collect();

    for relay in [relay_a.as_str(), relay_b.as_str()] {
        assert!(
            subscribe_relays.contains(&relay),
            "REQ (Subscribe) must be fanned to {relay}; got {subscribe_relays:?}"
        );
        assert!(
            sendframe_relays.contains(&relay),
            "connect EVENT (SendFrame) must be fanned to {relay}; got {sendframe_relays:?}"
        );
    }
}

// ─── BLOCKER 3 — non-canonical relay URI still matches inbound/reconnect ──────

/// A bunker URI with a non-canonical relay spelling (uppercase scheme/host +
/// trailing slash) must be canonicalized at the runtime boundary so inbound and
/// reconnect filtering matches the pool's canonical keys.
#[test]
fn non_canonical_relay_uri_matches_inbound() {
    let local_keys = Keys::generate();
    let bunker_keys = Keys::generate();
    let sub_id = "nip46-canon".to_string();

    let raw = "WSS://Relay.Example/".to_string();
    let canonical = canonical_relay_url(&raw).expect("relay URL must canonicalize");
    assert_ne!(
        raw, canonical,
        "test premise: raw spelling is non-canonical"
    );

    let handle = new_nip46_runtime_handle();
    init_bunker(
        &handle,
        sub_id,
        local_keys,
        bunker_keys.public_key(),
        vec![raw.clone()],
        None,
        None,
        now_unix_secs(),
    )
    .expect("init_bunker must succeed");

    // Stored relay list must be canonical.
    {
        let guard = handle.lock().unwrap();
        let rt = guard.as_ref().expect("runtime present");
        assert_eq!(
            rt.relay_urls(),
            &[canonical.clone()],
            "relay_urls must be canonicalized at the boundary"
        );
    }

    // on_relay_connected must match for BOTH the raw and the canonical spelling
    // (both sides are canonicalized), and must NOT match an unrelated relay.
    let now = now_unix_secs();
    let connect = |url: &str| -> usize {
        let mut guard = handle.lock().unwrap();
        guard
            .as_mut()
            .unwrap()
            .on_relay_connected(url, true, now)
            .len()
    };
    assert_eq!(connect(&raw), 1, "raw non-canonical spelling must match");
    assert_eq!(connect(&canonical), 1, "canonical spelling must match");
    assert_eq!(
        connect("wss://unrelated.relay"),
        0,
        "unrelated relay must not match"
    );
}

// ─── BLOCKER 1 — nostrconnect SignerReady updates the decode pubkey ───────────

/// `init_nostrconnect` stores the LOCAL pubkey as a placeholder remote pubkey
/// (the signer's key is unknown until the connect frame arrives).  Without the
/// `record_signer_ready` write-back, steady-state decode would decrypt with the
/// stale placeholder and drop every response.  This proves the write-back makes
/// the runtime's stored pubkey the one that decodes a real bunker response.
#[test]
fn nostrconnect_signer_ready_updates_decode_pubkey() {
    let local_keys = Keys::generate();
    let bunker_keys = Keys::generate(); // the signer app's key, learned at SignerReady
    let sub_id = "nip46-nc".to_string();
    let relay_url = "wss://nc.relay.example".to_string();

    let handle = new_nip46_runtime_handle();
    let (_uri, _effects) = init_nostrconnect(
        &handle,
        sub_id.clone(),
        local_keys.clone(),
        relay_url,
        "secret-xyz".to_string(),
        None,
        "T121",
        now_unix_secs(),
    )
    .expect("init_nostrconnect must succeed");

    // Placeholder: remote pubkey == local pubkey before SignerReady.
    {
        let guard = handle.lock().unwrap();
        let rt = guard.as_ref().unwrap();
        assert_eq!(
            rt.remote_pubkey(),
            local_keys.public_key(),
            "nostrconnect must store the local pubkey as a placeholder"
        );
    }

    // Build a real bunker→client response event (encrypted by the bunker to our
    // local key), as the mock relay would.
    let body = r#"{"id":"00000000002","result":"pong"}"#;
    let event_frame = build_event_frame(&bunker_keys, local_keys.public_key(), body)
        .expect("response event must build");
    let event_obj: serde_json::Value = {
        let arr: serde_json::Value = serde_json::from_str(&event_frame).unwrap();
        arr.as_array().unwrap()[1].clone()
    };

    // BEFORE write-back: decoding with the stored placeholder fails (the bug).
    {
        let placeholder = handle.lock().unwrap().as_ref().unwrap().remote_pubkey();
        assert!(
            nmp_nip46::decode_inbound_response(&event_obj, &local_keys, placeholder).is_none(),
            "placeholder pubkey must NOT decode the bunker response"
        );
    }

    // SignerReady write-back (the BLOCKER-1 fix).
    record_signer_ready(&handle, bunker_keys.public_key());
    {
        let guard = handle.lock().unwrap();
        assert_eq!(
            guard.as_ref().unwrap().remote_pubkey(),
            bunker_keys.public_key(),
            "record_signer_ready must persist the learned remote signer pubkey"
        );
    }

    // AFTER write-back: the stored pubkey now decodes the response.
    {
        let learned = handle.lock().unwrap().as_ref().unwrap().remote_pubkey();
        let decoded = nmp_nip46::decode_inbound_response(&event_obj, &local_keys, learned)
            .expect("learned pubkey must decode the bunker response");
        assert_eq!(decoded, body, "decoded body must round-trip");
    }
}

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Drive the NIP-46 handshake through the RUNTIME state machine until
/// [`Effect::SignerReady`] or timeout.
fn drive_runtime_handshake(
    handle: &Nip46RuntimeHandle,
    pool_rx: &Receiver<PoolEvent>,
    pool: &Pool,
    h: RelayHandle,
    relay_url: &str,
    timeout: Duration,
) -> Option<SignerReady> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .unwrap_or(Duration::ZERO);
        match pool_rx.recv_timeout(remaining.min(Duration::from_millis(200))) {
            Ok(PoolEvent::Frame {
                frame: RelayFrame::Text(text),
                ..
            }) => {
                let (effects, _decoded) = {
                    let mut guard = handle.lock().expect("runtime lock");
                    let rt = guard.as_mut()?;
                    rt.on_relay_text(relay_url, &text, now_unix_secs())
                };
                for effect in effects {
                    match effect {
                        Effect::SendFrame { text, .. } => {
                            pool.send(h, WireFrame::Text(text));
                        }
                        Effect::SignerReady(ready) => return Some(ready),
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
fn wait_opened(rx: &Receiver<PoolEvent>, timeout: Duration) -> Option<RelayHandle> {
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
