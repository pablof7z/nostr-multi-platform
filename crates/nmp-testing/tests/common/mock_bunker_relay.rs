//! Minimal plain-`ws://` relay server tailored for NIP-46 (bunker) integration
//! tests.
//!
//! Why a dedicated mock rather than reusing a generic one?
//!
//! - The wire envelope is unusual: kind:24133 ephemeral events with NIP-44
//!   encrypted JSON-RPC payloads tagged `["p", <local_pubkey>]`. A generic
//!   relay would have to grow NIP-46 decode logic anyway.
//! - The mock plays **both** the relay AND the remote signer. It owns
//!   `bunker_keys` (the bunker's npub) and `user_keys` (the user whose nsec is
//!   being held). It decrypts each `sign_event` RPC, signs the inner event
//!   with `user_keys`, and replies with an encrypted
//!   `{"id":...,"result":<signed-event-json>}` envelope encrypted back to the
//!   client's local pubkey.
//!
//! ## Threading model
//!
//! - Acceptor thread: loops on `TcpListener::accept` (set non-blocking + poll
//!   the shutdown flag with a 20ms sleep).
//! - Per-connection worker thread: WebSocket handshake + frame loop, with a
//!   100ms read timeout so it cycles back to check shutdown.
//!
//! On `Drop` we flip the shutdown atomic and drop the listener. Workers exit
//! within ~100ms.
//!
//! This is NOT a general-purpose nostr relay. It only understands `REQ` /
//! `EVENT` frames sufficient for the NIP-46 handshake + `sign_event` call
//! path used by [`nmp_signer_broker`].

use std::io::ErrorKind;
use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use nostr::nips::nip44;
use nostr::{EventBuilder, Keys, Kind, PublicKey, Tag, Timestamp};
use serde_json::{json, Value};

/// Handle to a running mock bunker relay. Drop to shut down.
pub struct MockBunkerRelay {
    addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    seen_methods: Arc<Mutex<Vec<String>>>,
    listener: Option<TcpListener>,
    acceptor: Mutex<Option<JoinHandle<()>>>,
    _workers: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl MockBunkerRelay {
    /// Spawn the relay on `127.0.0.1` (OS-picked port).
    ///
    /// `bunker_keys` plays the remote-signer side; the broker's parsed
    /// `bunker://<bunker_pubkey>?...` URI must use the matching `pub key`.
    /// `user_keys` is the user whose nsec is being custodied — the relay
    /// signs `sign_event` requests with these. `get_public_key` replies with
    /// `user_keys.public_key().to_hex()`.
    pub fn spawn(bunker_keys: Keys, user_keys: Keys) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let workers: Arc<Mutex<Vec<JoinHandle<()>>>> = Arc::new(Mutex::new(Vec::new()));

        let listener_for_thread = listener.try_clone()?;
        let shutdown_t = Arc::clone(&shutdown);
        let seen_t = Arc::clone(&seen);
        let workers_t = Arc::clone(&workers);
        let acceptor = std::thread::spawn(move || {
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
                        let bunker_keys = bunker_keys.clone();
                        let user_keys = user_keys.clone();
                        let shutdown_w = Arc::clone(&shutdown_t);
                        let seen_w = Arc::clone(&seen_t);
                        let worker = std::thread::spawn(move || {
                            run_connection(stream, bunker_keys, user_keys, shutdown_w, seen_w);
                        });
                        workers_t.lock().unwrap().push(worker);
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => return,
                }
            }
        });

        Ok(Self {
            addr,
            shutdown,
            seen_methods: seen,
            listener: Some(listener),
            acceptor: Mutex::new(Some(acceptor)),
            _workers: workers,
        })
    }

    /// `ws://127.0.0.1:<port>` — pass into `bunker://...?relay=<this>`.
    pub fn ws_url(&self) -> String {
        format!("ws://{}", self.addr)
    }

    /// Methods we've decrypted off the wire so far (`connect`, `get_public_key`,
    /// `sign_event`, …).
    pub fn observed_methods(&self) -> Vec<String> {
        self.seen_methods.lock().unwrap().clone()
    }
}

impl Drop for MockBunkerRelay {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        drop(self.listener.take()); // closes the listening socket
        if let Ok(mut guard) = self.acceptor.lock() {
            if let Some(j) = guard.take() {
                let _ = j.join();
            }
        }
    }
}

/// Per-connection worker: WebSocket upgrade, then loop reading text frames.
fn run_connection(
    stream: std::net::TcpStream,
    bunker_keys: Keys,
    user_keys: Keys,
    shutdown: Arc<AtomicBool>,
    seen: Arc<Mutex<Vec<String>>>,
) {
    let mut ws = match tungstenite::accept(stream) {
        Ok(w) => w,
        Err(_) => return,
    };
    let mut client_local_pubkey: Option<PublicKey> = None;
    let mut subscription_id: Option<String> = None;

    loop {
        if shutdown.load(Ordering::Relaxed) {
            let _ = ws.close(None);
            return;
        }
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
                if arr.len() < 3 {
                    continue;
                }
                if let Some(sub) = arr.get(1).and_then(|v| v.as_str()) {
                    subscription_id = Some(sub.to_string());
                }
                if let Some(filter) = arr.get(2) {
                    if let Some(p_arr) = filter.get("#p").and_then(|v| v.as_array()) {
                        if let Some(p_hex) = p_arr.first().and_then(|v| v.as_str()) {
                            if let Ok(pk) = PublicKey::from_hex(p_hex) {
                                client_local_pubkey = Some(pk);
                            }
                        }
                    }
                }
                if let Some(sub) = &subscription_id {
                    let eose = json!(["EOSE", sub]).to_string();
                    let _ = ws.send(tungstenite::Message::Text(eose));
                }
            }
            "EVENT" => {
                let event = match arr.get(1) {
                    Some(e) => e,
                    None => continue,
                };
                let Some(client_pk) = client_local_pubkey else {
                    continue;
                };
                let Some(response) =
                    handle_incoming_rpc(event, &bunker_keys, &user_keys, client_pk, &seen)
                else {
                    continue;
                };
                let sub_id = subscription_id.clone().unwrap_or_else(|| "0".to_string());
                let frame = json!(["EVENT", sub_id, response]).to_string();
                if ws.send(tungstenite::Message::Text(frame)).is_err() {
                    return;
                }
            }
            "CLOSE" => return,
            _ => {}
        }
    }
}

/// Process a single kind:24133 EVENT from the client. Returns the encrypted
/// response event to send back, or `None` if we couldn't decode (which would
/// be a fatal protocol error in production but harmless in the mock — the
/// broker will time out).
fn handle_incoming_rpc(
    event: &Value,
    bunker_keys: &Keys,
    user_keys: &Keys,
    client_pk: PublicKey,
    seen: &Arc<Mutex<Vec<String>>>,
) -> Option<Value> {
    let ciphertext = event.get("content").and_then(|v| v.as_str())?;
    let plaintext =
        nip44::decrypt(bunker_keys.secret_key(), &client_pk, ciphertext.as_bytes()).ok()?;
    let rpc: Value = serde_json::from_str(&plaintext).ok()?;
    let id = rpc.get("id").and_then(|v| v.as_str())?.to_string();
    let method = rpc.get("method").and_then(|v| v.as_str())?.to_string();
    seen.lock().unwrap().push(method.clone());

    let result_str: String = match method.as_str() {
        "connect" => "ack".to_string(),
        "get_public_key" => user_keys.public_key().to_hex(),
        "sign_event" => {
            // params: [<UnsignedEvent json>]
            let params = rpc.get("params").and_then(|v| v.as_array())?;
            let unsigned_value = params.first()?;
            let kind_u64 = unsigned_value.get("kind").and_then(|v| v.as_u64())?;
            let content = unsigned_value
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let created_at = unsigned_value
                .get("created_at")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let tag_rows: Vec<Vec<String>> = unsigned_value
                .get("tags")
                .and_then(|v| v.as_array())
                .map(|rows| {
                    rows.iter()
                        .filter_map(|row| {
                            row.as_array().map(|cells| {
                                cells
                                    .iter()
                                    .filter_map(|c| c.as_str().map(str::to_string))
                                    .collect::<Vec<_>>()
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let kind = Kind::from_u16(u16::try_from(kind_u64).ok()?);
            let tags: Vec<Tag> = tag_rows
                .iter()
                .filter_map(|row| Tag::parse(row).ok())
                .collect();
            let signed_event = EventBuilder::new(kind, content)
                .tags(tags)
                .custom_created_at(Timestamp::from(created_at))
                .sign_with_keys(user_keys)
                .ok()?;
            // NIP-46 sign_event returns the full signed event JSON as a
            // single result string.
            serde_json::to_string(&signed_event).ok()?
        }
        _ => {
            let resp = json!({
                "id": id,
                "result": "",
                "error": format!("unknown method: {method}"),
            });
            return build_response_event(bunker_keys, client_pk, resp);
        }
    };

    let response_rpc = json!({"id": id, "result": result_str});
    build_response_event(bunker_keys, client_pk, response_rpc)
}

/// NIP-44-encrypt the RPC response and wrap as a kind:24133 event signed by
/// the bunker, tagged `["p", <client_pubkey>]`.
fn build_response_event(
    bunker_keys: &Keys,
    client_pk: PublicKey,
    response_rpc: Value,
) -> Option<Value> {
    let body = response_rpc.to_string();
    let ciphertext = nip44::encrypt(
        bunker_keys.secret_key(),
        &client_pk,
        body.as_bytes(),
        nip44::Version::V2,
    )
    .ok()?;
    let event = EventBuilder::new(Kind::from_u16(24133), ciphertext)
        .tags(vec![Tag::parse(["p", &client_pk.to_hex()]).ok()?])
        .sign_with_keys(bunker_keys)
        .ok()?;
    serde_json::to_value(&event).ok()
}
