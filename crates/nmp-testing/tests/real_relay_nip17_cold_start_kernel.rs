//! F-02 closure-gate: cold-start DM receive driven through a REAL
//! `NmpApp`/Kernel (issue #977).
//!
//! ## Why this test exists
//!
//! `real_relay_nip17_cold_start.rs` (PR #1073) proved the cold-start
//! *transport* + *projection-decrypt* halves by driving
//! `DmInboxProjection::on_raw_event_with_source` directly. The Opus review of
//! that PR adjudicated that ONE seam remained uncovered by any executing test:
//!
//! > a live-relay REQ driven through a real kernel's planner-compiled kind:1059
//! > `#p` subscription (kind:10050 → `DmRelayCache` → planner routing) with the
//! > Schnorr-verify + store-insert gate in the loop.
//!
//! This test closes that gate. NO part of the subscription is hand-rolled: the
//! kernel itself compiles and emits the kind:1059 `#p` REQ over a real
//! WebSocket, ingests the relay's backfill through its verify+store gate, fans
//! it to the registered `DmInboxProjection`, and surfaces it in the
//! `"nmp.nip17.dm_inbox"` snapshot.
//!
//! ## Why this test uses a public `wss://` relay instead of `nak serve`
//!
//! `nak serve` (the deterministic in-process relay used by
//! `real_relay_nip17_cold_start.rs`) only supports plain `ws://` connections.
//! The production `Kind10050Parser` intentionally rejects any `ws://` relay
//! URL for security reasons (a plain-WebSocket DM relay would degrade
//! NIP-59 sealed gift-wrap confidentiality). Because `ws://` is rejected,
//! the `DmRelayCache` would never be populated from a nak-served kind:10050,
//! and the F-02 kernel path would fail-closed regardless of the fix — the
//! nak-serve scenario is physically incapable of exercising this gate.
//!
//! `relay.primal.net` (TLS, `wss://`) is the right oracle: it accepts
//! ephemeral random-key events and can store/replay them.
//!
//! ## The scenario (the production cold-start path, end to end)
//!
//! 1. **Bob is offline.** Two events are published to `relay.primal.net`
//!    over raw WebSockets while no kernel exists:
//!    - Bob's own kind:10050 DM-relay-list naming `relay.primal.net`. (A real
//!      returning user already has this published from a prior device — exactly
//!      the case `startup.rs`'s F-02 comment calls out.)
//!    - Alice's gift-wrap: a kind:14 rumor sealed into a kind:1059 addressed
//!      to Bob (`#p`).
//! 2. **Bob cold-starts a real `NmpApp`.** `nmp_app_new` →
//!    `nmp_defaults::register_defaults` (wires the kind:10050
//!    `Kind10050Parser` + `DmRelayCache`, the `DmInboxProjection`, and the
//!    `DmRuntimeController` reconciler — the exact composition Chirp ships) →
//!    `nmp_app_start` → `nmp_app_add_relay(relay, "both,indexer")` →
//!    `nmp_app_signin_nsec(bob)`.
//! 3. **The kernel does the rest, unaided:**
//!    - On sign-in the kernel fires the active-account bootstrap, which fetches
//!      Bob's kind:10050 as a OneShot against the indexer relay.
//!    - The relay backfills Bob's kind:10050; `Kind10050Parser` writes the
//!      `wss://relay.primal.net` URL into the shared `DmRelayCache`.
//!    - The `DmRuntimeController`'s per-tick reconcile observes the active
//!      account + read relays and pushes
//!      `active_giftwrap_inbox_interest(bob)` (kind:1059 `#p bob`,
//!      `PTagRouting::Nip17DmRelays`).
//!    - The F-02 fix: the wildcard ingest arm detects the `DmRelayCache`
//!      transition (before = None, after = Some([...])) and enqueues a
//!      `DmRelayListChanged` trigger. On the next `drain_lifecycle_tick`
//!      the planner recompiles the interest and routes it to
//!      `wss://relay.primal.net`.
//!    - The relay backfills the stored gift-wrap; the kernel verifies its
//!      Schnorr signature, inserts it into the store, and fans it to the
//!      `DmInboxProjection` raw-event observer.
//! 4. **Assertion.** Block on the kernel's update-callback channel (D8 — no
//!    sleep+poll) and re-read the `"nmp.nip17.dm_inbox"` snapshot on each tick
//!    until the decrypted DM lands (peer = Alice, content verbatim), or the
//!    deadline passes.
//!
//! ## Running manually
//!
//! ```bash
//! cargo test -p nmp-testing \
//!   --test real_relay_nip17_cold_start_kernel \
//!   -- nip17_cold_start_receive_through_real_kernel --ignored --nocapture
//! ```

use std::ffi::{c_void, CString};
use std::net::TcpStream;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::sync::Arc;
use std::time::{Duration, Instant};

use nmp_native_runtime::NmpApp;
use nmp_nip59::gift_wrap_local;
use nostr::nips::nip59::RANGE_RANDOM_TIMESTAMP_TWEAK;
use nostr::util::JsonUtil as _;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp, ToBech32 as _};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

// ─── Constants ─────────────────────────────────────────────────────────────

/// The relay used for the F-02 closure-gate test.
///
/// Must be `wss://` because `Kind10050Parser` rejects `ws://` relay URLs for
/// security reasons (plain-WebSocket DM relay degrades NIP-59 sealed
/// gift-wrap confidentiality). `nak serve` is `ws://`-only and therefore
/// cannot exercise this gate.
const RELAY: &str = "wss://relay.primal.net";

const READ_TIMEOUT: Duration = Duration::from_millis(250);
const PUBLISH_ACK_BUDGET: Duration = Duration::from_secs(10);
/// Budget for the whole kernel round-trip: sign-in → kind:10050 fetch →
/// DmRelayCache write → DmRelayListChanged trigger → planner REQ → backfill
/// → verify+store → projection. Generous: the relay is a real public relay
/// with network latency.
const KERNEL_DELIVERY_BUDGET: Duration = Duration::from_secs(30);

// ─── TLS ────────────────────────────────────────────────────────────────────

fn install_rustls_provider() {
    use std::sync::Once;
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

type RelaySocket = WebSocket<MaybeTlsStream<TcpStream>>;

fn open(url: &str) -> Result<RelaySocket, String> {
    install_rustls_provider();
    let (mut socket, _response) = connect(url).map_err(|e| e.to_string())?;
    match socket.get_mut() {
        MaybeTlsStream::Plain(s) => {
            let _ = s.set_read_timeout(Some(READ_TIMEOUT));
        }
        MaybeTlsStream::Rustls(s) => {
            let _ = s.get_ref().set_read_timeout(Some(READ_TIMEOUT));
        }
        #[allow(unreachable_patterns)]
        _ => {}
    }
    Ok(socket)
}

/// Publish one signed event and wait for the relay `OK`.
/// Returns `Ok(true)` on success, `Ok(false)` if the relay is unreachable
/// (SKIP), `Err` on an explicit relay reject.
fn publish_and_ack(event_json: &str, event_id: &str) -> Result<bool, String> {
    let mut sock = match open(RELAY) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[f02-kernel] SKIP: cannot reach {RELAY}: {e}");
            return Ok(false);
        }
    };
    sock.send(Message::Text(format!("[\"EVENT\",{event_json}]")))
        .map_err(|e| format!("send EVENT: {e}"))?;

    let deadline = Instant::now() + PUBLISH_ACK_BUDGET;
    while Instant::now() < deadline {
        match sock.read() {
            Ok(Message::Text(text)) => {
                if text.contains("\"OK\"") && text.contains(event_id) {
                    let _ = sock.close(None);
                    if text.contains("true") {
                        return Ok(true);
                    }
                    return Err(format!("relay rejected publish: {text}"));
                }
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => {
                let _ = sock.close(None);
                eprintln!("[f02-kernel] SKIP: socket error during publish: {e}");
                return Ok(false);
            }
        }
    }
    let _ = sock.close(None);
    eprintln!("[f02-kernel] SKIP: no OK within {PUBLISH_ACK_BUDGET:?}");
    Ok(false)
}

// ─── Relay reachability probe ────────────────────────────────────────────────

/// Check whether the relay is reachable via TCP. Returns `false` if we should
/// SKIP (no network, relay down, etc.).
fn relay_reachable() -> bool {
    // Extract host:port from the wss:// URL.
    let host = RELAY
        .trim_start_matches("wss://")
        .trim_start_matches("ws://");
    let (host, port) = if let Some(pos) = host.find(':') {
        (&host[..pos], host[pos + 1..].parse::<u16>().unwrap_or(443))
    } else {
        (host, 443u16)
    };
    // Use ToSocketAddrs to resolve the hostname, then try connecting.
    let Ok(mut addrs) = std::net::ToSocketAddrs::to_socket_addrs(&(host, port)) else {
        return false;
    };
    addrs.any(|addr| TcpStream::connect_timeout(&addr, Duration::from_secs(5)).is_ok())
}

// ─── Kernel update-callback signal (D8 blocking-wait, no sleep+poll) ────────
//
// `extern "C"` callbacks cannot capture, so the update `Sender` is parked in a
// process-global slot and a tick is forwarded on every kernel update frame.
// Mirrors the proven pattern in
// `crates/nmp-ffi/src/active_account_handle_tests.rs`.

static UPDATE_TX: OnceLock<Mutex<Option<Sender<()>>>> = OnceLock::new();

extern "C" fn update_signal_callback(_ctx: *mut c_void, _ptr: *const u8, _len: usize) {
    if let Some(slot) = UPDATE_TX.get() {
        if let Ok(guard) = slot.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(());
            }
        }
    }
}

fn install_update_signal() -> Receiver<()> {
    let (tx, rx) = channel::<()>();
    let slot = UPDATE_TX.get_or_init(|| Mutex::new(None));
    *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(tx);
    rx
}

fn uninstall_update_signal() {
    if let Some(slot) = UPDATE_TX.get() {
        *slot.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
}

/// Read the `"nmp.nip17.dm_inbox"` typed FlatBuffers projection from the live kernel.
/// The generic JSON lane is deleted (rule A6).
fn dm_inbox_snapshot(app: *mut NmpApp) -> Option<nmp_nip17::DmInboxSnapshot> {
    let app_ref: &NmpApp = unsafe { &*app };
    let projections = app_ref.run_typed_snapshot_projections();
    let entry = projections.iter().find(|p| p.key == "nmp.nip17.dm_inbox" && !p.payload.is_empty())?;
    nmp_nip17::wire::dm_inbox_fb::decode_dm_inbox_snapshot(&entry.payload).ok()
}

/// Extract the single conversation's first message `(peer_pubkey, content)`
/// from a `DmInboxSnapshot` struct, if a decrypted message is present.
fn first_message(snapshot: &nmp_nip17::DmInboxSnapshot) -> Option<(String, String)> {
    let convo = snapshot.conversations.first()?;
    let peer = convo.peer_pubkey.clone();
    let msg = convo.messages.first()?;
    let content = msg.content.clone();
    Some((peer, content))
}

/// Block on kernel update ticks, re-reading the DM-inbox snapshot on each,
/// until a decrypted message appears or `budget` elapses.
///
/// D8-compliant: the steady state is a blocking `recv_timeout` on the kernel's
/// update channel, NOT a sleep+poll loop. The per-recv timeout only bounds a
/// hung actor.
fn wait_for_dm(
    rx: &Receiver<()>,
    app: *mut NmpApp,
    budget: Duration,
) -> Result<(String, String), String> {
    let deadline = Instant::now() + budget;
    // Initial check (a tick may have landed before we started waiting).
    if let Some(snap) = dm_inbox_snapshot(app) {
        if let Some(found) = first_message(&snap) {
            return Ok(found);
        }
    }
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(remaining.min(Duration::from_secs(2))) {
            Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if let Some(snap) = dm_inbox_snapshot(app) {
                    if let Some(found) = first_message(&snap) {
                        return Ok(found);
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err("kernel update channel disconnected".to_string());
            }
        }
    }
    Err(format!(
        "DM did not surface in nmp.nip17.dm_inbox within {budget:?}"
    ))
}

// ─── The closure-gate test ──────────────────────────────────────────────────

/// NmpApp instances spawn process-global actor threads; serialise with the
/// shared update-signal slot.
static SERIAL: Mutex<()> = Mutex::new(());

#[test]
#[ignore = "real-relay (wss://relay.primal.net): run with --ignored --nocapture"]
fn nip17_cold_start_receive_through_real_kernel() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());

    // Reachability pre-check — skip gracefully if the relay is down.
    if !relay_reachable() {
        eprintln!("[f02-kernel] SKIP: {RELAY} unreachable");
        return;
    }

    let outcome = run_scenario();

    match outcome {
        Ok(true) => println!("[f02-kernel] F-02 CLOSURE-GATE: PASS"),
        Ok(false) => eprintln!("[f02-kernel] SKIP (relay unreachable during scenario)"),
        Err(msg) => panic!("{msg}"),
    }
}

/// Returns `Ok(true)` on success, `Ok(false)` on a graceful SKIP (relay
/// unreachable), `Err` on a real scenario failure.
fn run_scenario() -> Result<bool, String> {
    // ── Identities (fresh random keys — no pubkey reuse) ─────────────────
    let alice = Keys::generate();
    let bob = Keys::generate();
    let bob_nsec = bob
        .secret_key()
        .to_bech32()
        .map_err(|e| format!("bob nsec encode: {e}"))?;
    let alice_hex = alice.public_key().to_hex();
    println!("[f02-kernel] alice (sender):    {alice_hex}");
    println!("[f02-kernel] bob   (recipient): {}", bob.public_key().to_hex());
    println!("[f02-kernel] relay:             {RELAY}");

    // ── PHASE 1: publish while Bob is offline ───────────────────────────────
    //
    // (a) Bob's own kind:10050 DM-relay-list naming THIS relay (wss://). A real
    //     returning user already has this published from a prior device — the
    //     exact case `startup.rs`'s F-02 comment calls out.
    let bob_relay_list = EventBuilder::new(Kind::from_u16(10050), "")
        .tag(Tag::custom(
            nostr::TagKind::custom("relay"),
            [RELAY.to_string()],
        ))
        .custom_created_at(Timestamp::now())
        .sign_with_keys(&bob)
        .map_err(|e| format!("sign bob kind:10050: {e}"))?;
    let bob_relay_list_id = bob_relay_list.id.to_hex();
    println!("[f02-kernel] PHASE 1a: publishing Bob's kind:10050 id={bob_relay_list_id}");
    match publish_and_ack(&bob_relay_list.as_json(), &bob_relay_list_id)? {
        true => {}
        false => return Ok(false),
    }

    // (b) Alice's gift-wrap: kind:14 rumor → kind:1059 addressed to Bob (`#p`).
    let plaintext = format!(
        "f02-closure-gate: hello bob — ts={}",
        Timestamp::now().as_secs()
    );
    let rumor = EventBuilder::new(Kind::from_u16(14), &plaintext)
        .tag(Tag::public_key(bob.public_key()))
        .custom_created_at(Timestamp::now())
        .build(alice.public_key());
    let tweaked = Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK);
    let envelope = gift_wrap_local(&alice, &bob.public_key(), &rumor, tweaked)
        .map_err(|e| format!("gift_wrap_local: {e}"))?;
    assert_eq!(envelope.kind, Kind::GiftWrap, "outer kind must be 1059");
    let envelope_id = envelope.id.to_hex();
    println!("[f02-kernel] PHASE 1b: publishing Alice's kind:1059 id={envelope_id} (Bob offline)");
    match publish_and_ack(&envelope.as_json(), &envelope_id)? {
        true => {}
        false => return Ok(false),
    }
    println!("[f02-kernel] relay stored both events — Bob's kernel does not exist yet");

    // ── PHASE 2: Bob cold-starts a REAL kernel ──────────────────────────────
    let rx = install_update_signal();
    let app = Box::into_raw(Box::new(nmp_native_runtime::new_app()));
    unsafe { &*app }.set_update_listener(Some(std::sync::Arc::new(|bytes: &[u8]| {
        update_signal_callback(std::ptr::null_mut(), bytes.as_ptr(), bytes.len());
    })));

    // The canonical NMP composition (exactly what Chirp's
    // `nmp_app_chirp_register` inherits): kind:10050 `Kind10050Parser` +
    // `DmRelayCache`, `DmInboxProjection`, and the `DmRuntimeController`
    // reconciler that pushes the gift-wrap inbox interest on sign-in.
    //
    // SAFETY: `app` is a live pointer from `nmp_app_new`; the exclusive borrow
    // is released before any other access.
    nmp_defaults::register_defaults(unsafe { &mut *app });

    // Start the actor + real relay-worker pool, then add the relay as
    // read+write+indexer:
    //   - `indexer` so the active-account bootstrap's kind:10050 OneShot lands
    //     here.
    //   - `read` so the DmRuntimeController's reconcile observes a non-empty
    //     read-relay set and pushes the inbox interest.
    unsafe { &*app }.start_runtime(256, 8); // emit_hz=8 → ~125ms snapshot cadence
    unsafe { &*app }.add_relay(RELAY.to_owned(), "both,indexer".to_owned());

    // Sign Bob in. This is the trigger: the kernel fetches Bob's kind:10050,
    // populates the DmRelayCache (the F-02 fix detects this transition and
    // enqueues DmRelayListChanged), and the reconciler pushes the kind:1059
    // `#p` interest — all driven by the kernel, no hand-rolled REQ.
    unsafe { &*app }.signin_nsec_for_test(bob_nsec, true);
    println!(
        "[f02-kernel] PHASE 2: Bob signed in — kernel drives kind:10050 bootstrap → \
         DmRelayListChanged trigger → planner REQ → backfill"
    );

    // ── PHASE 3: assert the DM lands through the full kernel path ───────────
    let result = wait_for_dm(&rx, app, KERNEL_DELIVERY_BUDGET);

    // Teardown regardless of outcome.
    unsafe { &*app }.set_update_listener(None);
    unsafe { drop(Box::from_raw(app)) };
    uninstall_update_signal();

    let (peer, content) = result?;
    println!("[f02-kernel] PHASE 3: DM surfaced in nmp.nip17.dm_inbox snapshot");
    println!("[f02-kernel]   peer_pubkey: {peer}");
    println!("[f02-kernel]   content:     {content:?}");

    if peer != alice_hex {
        return Err(format!(
            "decrypted DM peer must be Alice ({alice_hex}), got {peer}"
        ));
    }
    if content != plaintext {
        return Err(format!(
            "decrypted content must round-trip verbatim: expected {plaintext:?}, got {content:?}"
        ));
    }
    Ok(true)
}
