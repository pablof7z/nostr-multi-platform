//! Mock NIP-46 infrastructure for nostrconnect:// integration tests.
//!
//! ## Architecture
//!
//! The nostrconnect:// flow requires **two** WebSocket clients (broker + signer
//! app) to communicate through a relay. This module provides:
//!
//! 1. `MockNostrConnectRelay` — a minimal broadcast relay that accepts multiple
//!    WebSocket connections and fans out every EVENT to all subscribers.
//!
//! 2. `MockNostrConnectSigner` — a signer-app simulator that:
//!    a. Spawns `MockNostrConnectRelay` internally.
//!    b. Exposes `connect_with_correct_secret(uri)` / `connect_with_wrong_secret(uri, bad)`.
//!    c. When called, dials the relay as a WebSocket client (playing the signer
//!       app), sends a NIP-44-encrypted `connect` RPC with params
//!       `[signer_pubkey, secret, ""]`, then waits for the broker's
//!       `get_public_key` and replies with `user_keys.public_key().to_hex()`.
//!
//! ## Threading model
//!
//! The relay runs an acceptor thread + per-connection worker threads. Workers
//! share a broadcast channel via `Arc<Mutex<Vec<Sender<String>>>>` so any
//! published EVENT is forwarded to all connected subscribers.
//!
//! On `Drop`, the shutdown flag is set; workers exit within ~100ms.

use std::io::ErrorKind;
use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use nostr::nips::nip44;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
use serde_json::{json, Value};

/// Shared broadcast state: list of per-connection event senders.
type BroadcastSenders = Arc<Mutex<Vec<Sender<String>>>>;

/// Mock NIP-46 relay + signer-app combo.
pub struct MockNostrConnectSigner {
    addr: SocketAddr,
    user_keys: Keys,
    shutdown: Arc<AtomicBool>,
    broadcast_senders: BroadcastSenders,
    listener: Option<TcpListener>,
    acceptor: Mutex<Option<JoinHandle<()>>>,
    workers: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl MockNostrConnectSigner {
    /// Spawn the mock relay on `127.0.0.1` (OS-picked port).
    pub fn spawn(user_keys: Keys) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let workers: Arc<Mutex<Vec<JoinHandle<()>>>> = Arc::new(Mutex::new(Vec::new()));
        let broadcast_senders: BroadcastSenders = Arc::new(Mutex::new(Vec::new()));

        let listener_for_thread = listener.try_clone()?;
        let shutdown_t = Arc::clone(&shutdown);
        let workers_t = Arc::clone(&workers);
        let senders_t = Arc::clone(&broadcast_senders);

        let acceptor = thread::spawn(move || {
            listener_for_thread
                .set_nonblocking(true)
                .expect("nonblocking accept");
            loop {
                if shutdown_t.load(Ordering::Relaxed) {
                    return;
                }
                match listener_for_thread.accept() {
                    Ok((stream, _peer)) => {
                        stream.set_nonblocking(false).expect("client to blocking");
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
                        let shutdown_w = Arc::clone(&shutdown_t);
                        let senders_w = Arc::clone(&senders_t);
                        let worker = thread::spawn(move || {
                            run_relay_connection(stream, shutdown_w, senders_w);
                        });
                        workers_t.lock().unwrap().push(worker);
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => return,
                }
            }
        });

        Ok(Self {
            addr,
            user_keys,
            shutdown,
            broadcast_senders,
            listener: Some(listener),
            acceptor: Mutex::new(Some(acceptor)),
            workers,
        })
    }

    /// `ws://127.0.0.1:<port>`.
    pub fn ws_url(&self) -> String {
        format!("ws://{}", self.addr)
    }

    /// Parse the nostrconnect URI, dial the relay, send `connect` with the
    /// correct secret, answer `get_public_key` with the user pubkey.
    pub fn connect_with_correct_secret(&self, uri: &str) {
        let params = parse_nostrconnect_uri(uri).expect("URI must be valid nostrconnect://");
        let secret = params.secret.clone();
        self.drive_signer_handshake(params, &secret);
    }

    /// Like `connect_with_correct_secret` but substitutes `bad_secret`.
    pub fn connect_with_wrong_secret(&self, uri: &str, bad_secret: &str) {
        let params = parse_nostrconnect_uri(uri).expect("URI must be valid nostrconnect://");
        self.drive_signer_handshake(params, bad_secret);
    }

    /// Spawn a driver thread that acts as the signer app dialing the relay.
    fn drive_signer_handshake(&self, params: NostrConnectParams, secret_to_send: &str) {
        let signer_keys = Keys::generate();
        let user_keys = self.user_keys.clone();
        let secret = secret_to_send.to_string();
        let relay_url = params.relay_url.clone();
        let client_pubkey = params.client_pubkey.clone();
        let workers = Arc::clone(&self.workers);

        let handle = thread::spawn(move || {
            // Small delay so the broker's REQ subscription is in place.
            thread::sleep(Duration::from_millis(150));

            let (mut ws, _) = match tungstenite::connect(&relay_url) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("mock signer: connect to relay failed: {e}");
                    return;
                }
            };

            let client_pk = match nostr::PublicKey::from_hex(&client_pubkey) {
                Ok(pk) => pk,
                Err(e) => {
                    eprintln!("mock signer: invalid client pubkey: {e}");
                    return;
                }
            };

            // Build the connect RPC: params = [signer_pubkey, secret, ""].
            let connect_id = format!("nc-connect-{}", &client_pubkey[..8]);
            let rpc = json!({
                "id": connect_id,
                "method": "connect",
                "params": [signer_keys.public_key().to_hex(), &secret, ""],
            });
            let Some(event_json) =
                build_encrypted_event(&signer_keys, client_pk, rpc, &client_pubkey)
            else {
                eprintln!("mock signer: build connect event failed");
                return;
            };

            // Send ["EVENT", <event>].
            if ws
                .send(tungstenite::Message::Text(format!(r#"["EVENT",{event_json}]"#)))
                .is_err()
            {
                eprintln!("mock signer: send connect event failed");
                return;
            }

            // Wait for broker's get_public_key (it arrives via relay broadcast
            // as ["EVENT", sub_id, {kind:24133,...}]).
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            let mut sent_gpk_reply = false;

            while std::time::Instant::now() < deadline {
                let msg = match ws.read() {
                    Ok(m) => m,
                    Err(tungstenite::Error::Io(io_err))
                        if matches!(
                            io_err.kind(),
                            ErrorKind::WouldBlock | ErrorKind::TimedOut
                        ) =>
                    {
                        continue;
                    }
                    Err(e) => {
                        eprintln!("mock signer: relay read failed: {e}");
                        break;
                    }
                };
                let text = match msg {
                    tungstenite::Message::Text(t) => t,
                    tungstenite::Message::Ping(p) => {
                        let _ = ws.send(tungstenite::Message::Pong(p));
                        continue;
                    }
                    tungstenite::Message::Close(_) => break,
                    _ => continue,
                };

                let parsed: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let arr = match parsed.as_array() {
                    Some(a) => a,
                    None => continue,
                };

                if arr.first().and_then(|v| v.as_str()) != Some("EVENT") {
                    continue;
                }
                let Some(inbound_event) = arr.get(2) else {
                    continue;
                };
                let Some(content) = inbound_event.get("content").and_then(|v| v.as_str()) else {
                    continue;
                };

                // Decrypt with signer_keys.secret + client_pk (broker sends
                // to signer, encrypted to signer's pubkey).
                let plaintext = match nip44::decrypt(
                    signer_keys.secret_key(),
                    &client_pk,
                    content.as_bytes(),
                ) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                let rpc_val: Value = match serde_json::from_str(&plaintext) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let rpc_id = rpc_val
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let method = rpc_val.get("method").and_then(|v| v.as_str()).unwrap_or("");

                if method == "get_public_key" {
                    let response = json!({
                        "id": rpc_id,
                        "result": user_keys.public_key().to_hex(),
                    });
                    if let Some(reply) =
                        build_encrypted_event(&signer_keys, client_pk, response, &client_pubkey)
                    {
                        let _ = ws.send(tungstenite::Message::Text(format!(
                            r#"["EVENT",{reply}]"#
                        )));
                    }
                    sent_gpk_reply = true;
                    break;
                }
            }

            if !sent_gpk_reply {
                eprintln!("mock signer: never received get_public_key from broker within 10s");
            }
        });

        workers.lock().unwrap().push(handle);
    }
}

impl Drop for MockNostrConnectSigner {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        drop(self.listener.take());
        if let Ok(mut guard) = self.acceptor.lock() {
            if let Some(j) = guard.take() {
                let _ = j.join();
            }
        }
    }
}

// ─── Broadcast relay connection handler ─────────────────────────────────────

/// Per-connection relay handler. Accepts REQ (registers subscription + sends
/// EOSE) and EVENT (broadcasts to all registered senders so every subscriber
/// receives the event).
fn run_relay_connection(
    stream: std::net::TcpStream,
    shutdown: Arc<AtomicBool>,
    broadcast_senders: BroadcastSenders,
) {
    let mut ws = match tungstenite::accept(stream) {
        Ok(w) => w,
        Err(_) => return,
    };

    // Register a broadcast receiver for this connection.
    let (tx, rx) = mpsc::channel::<String>();
    broadcast_senders.lock().unwrap().push(tx);

    let mut subscription_id: Option<String> = None;

    loop {
        if shutdown.load(Ordering::Relaxed) {
            let _ = ws.close(None);
            return;
        }

        // Drain broadcast messages and forward to this client.
        while let Ok(frame) = rx.try_recv() {
            if ws.send(tungstenite::Message::Text(frame)).is_err() {
                return;
            }
        }

        // Read one inbound frame (short timeout).
        let msg = match ws.read() {
            Ok(m) => m,
            Err(tungstenite::Error::Io(io_err))
                if matches!(io_err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                continue;
            }
            Err(_) => return,
        };

        let text = match msg {
            tungstenite::Message::Text(t) => t,
            tungstenite::Message::Ping(p) => {
                let _ = ws.send(tungstenite::Message::Pong(p));
                continue;
            }
            tungstenite::Message::Close(_) => return,
            _ => continue,
        };

        let parsed: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let arr = match parsed.as_array() {
            Some(a) => a,
            None => continue,
        };

        let kind_str = arr.first().and_then(|v| v.as_str()).unwrap_or("");
        match kind_str {
            "REQ" => {
                if let Some(sub) = arr.get(1).and_then(|v| v.as_str()) {
                    subscription_id = Some(sub.to_string());
                }
                // Reply EOSE so the broker knows the subscription is active.
                if let Some(sub) = &subscription_id {
                    let eose = json!(["EOSE", sub]).to_string();
                    let _ = ws.send(tungstenite::Message::Text(eose));
                }
            }
            "EVENT" => {
                let event = match arr.get(1) {
                    Some(e) => e.clone(),
                    None => continue,
                };
                let event_id = event.get("id").and_then(|v| v.as_str()).unwrap_or("?");

                // Broadcast to ALL connections (broker + signer will both receive it).
                let sub_id = subscription_id.clone().unwrap_or_else(|| "0".to_string());
                let broadcast_frame = json!(["EVENT", sub_id, event]).to_string();
                let senders = broadcast_senders.lock().unwrap();
                for s in senders.iter() {
                    let _ = s.send(broadcast_frame.clone());
                }
                drop(senders);

                // Also reply OK to the sender.
                let ok_frame = json!(["OK", event_id, true, ""]).to_string();
                let _ = ws.send(tungstenite::Message::Text(ok_frame));
            }
            "CLOSE" => return,
            _ => {}
        }
    }
}

// ─── Parsed nostrconnect:// URI ───────────────────────────────────────────────

struct NostrConnectParams {
    client_pubkey: String,
    secret: String,
    relay_url: String,
}

fn parse_nostrconnect_uri(uri: &str) -> Option<NostrConnectParams> {
    let rest = uri.strip_prefix("nostrconnect://")?;
    let (pubkey_raw, query_raw) = match rest.find('?') {
        Some(idx) => (&rest[..idx], &rest[idx + 1..]),
        None => (rest, ""),
    };

    let client_pubkey = pubkey_raw.to_ascii_lowercase();
    if client_pubkey.len() != 64 || !client_pubkey.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }

    let mut secret = String::new();
    let mut relay_url = String::new();

    for pair in query_raw.split('&') {
        if let Some(idx) = pair.find('=') {
            let k = &pair[..idx];
            let v = percent_decode(&pair[idx + 1..]);
            match k {
                "secret" => secret = v,
                "relay" => relay_url = v,
                _ => {}
            }
        }
    }

    if relay_url.is_empty() || secret.is_empty() {
        return None;
    }

    Some(NostrConnectParams {
        client_pubkey,
        secret,
        relay_url,
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(b);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_default()
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ─── NIP-44 helpers ───────────────────────────────────────────────────────────

fn build_encrypted_event(
    signer_keys: &Keys,
    recipient_pk: nostr::PublicKey,
    rpc: Value,
    recipient_pubkey_hex: &str,
) -> Option<String> {
    let body = rpc.to_string();
    let ciphertext = nip44::encrypt(
        signer_keys.secret_key(),
        &recipient_pk,
        body.as_bytes(),
        nip44::Version::V2,
    )
    .ok()?;
    let event = EventBuilder::new(Kind::from_u16(24133), ciphertext)
        .tags(vec![Tag::parse(["p", recipient_pubkey_hex]).ok()?])
        .custom_created_at(Timestamp::from(now_secs()))
        .sign_with_keys(signer_keys)
        .ok()?;
    serde_json::to_string(&event).ok()
}

fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
