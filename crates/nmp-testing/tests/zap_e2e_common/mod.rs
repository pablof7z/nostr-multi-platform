//! Shared harness for the F-04 zap end-to-end runtime verification (#978).
//!
//! Three pieces, each a thin test-side process/thread that lets the real
//! kernel pipeline run against controllable counterparties with zero real
//! money:
//!
//! 1. [`NakRelay`] — launches `nak serve` (an in-memory Nostr relay) on an
//!    OS-assigned free port and exposes its `ws://` URL. Drops the child
//!    process on `Drop`. This is the live relay both the kernel actor's
//!    `RelayRole::Wallet` socket AND the fake wallet connect to.
//!
//! 2. [`FakeNwcWallet`] — a scripted NIP-47 wallet *service*. It opens a
//!    tungstenite WebSocket to the same `nak serve` relay, `REQ`s for
//!    kind:23194 requests addressed to its wallet pubkey (`#p`), decrypts
//!    each `pay_invoice` request with the wallet secret (the canonical
//!    `nmp_nwc::crypto` path — NIP-04), and publishes a kind:23195 success
//!    response tagged `["e", <request_id>]` and encrypted back to the client
//!    pubkey. This is exactly the wire contract `nmp_nip47::handle_nwc_text`
//!    matches against (see `crates/nmp-nwc/src/decode.rs::
//!    try_decode_response_for_request`).
//!
//! 3. [`publish_zap_receipt`] / [`signed_zap_receipt_json`] — builds a real
//!    Schnorr-signed kind:9735 zap receipt (NIP-57 Appendix E shape) and
//!    publishes it to the relay (or returns its JSON for direct kernel
//!    injection).
//!
//! # Why no LNURL stub here
//!
//! The LNURL-pay leg (`nmp_nip57::lnurl::fetch_lnurl_invoice_blocking`) does
//! its two HTTP hops with `ureq` configured for TLS against the webpki root
//! store, and the code rejects any non-`https://` callback URL (LUD-01 §1).
//! A local stub would need a publicly-trusted certificate, so that single hop
//! is **not** mockable in-process. The harness therefore drives the NWC half
//! (the part that crosses a relay) at runtime and documents the LNURL hop as
//! the residual real-wallet last mile (see `real_wallet_zap_e2e`). The NWC
//! pay path and the receipt-ingest path together cover every kernel-side
//! state transition the zap pipeline makes.
//!
//! # D8 — no polling
//!
//! Threads here use blocking `recv`/socket reads with a wall-clock deadline,
//! never a `sleep`+check spin. The fake wallet loops on a blocking socket
//! read; the test side blocks on an mpsc `recv_timeout` fed by the
//! action-result observer callback.

// Each sibling test binary (zap_e2e_nwc_roundtrip / zap_e2e_real_wallet)
// compiles this module independently and uses a different subset of its
// helpers, so unused-item and unused-re-export warnings are expected and
// intentional here.
#![allow(dead_code, unused_imports)]

mod ffi_driver;
pub use ffi_driver::{
    build_app_signed_in, install_emit_signal, install_rustls_provider, nwc_uri, read_projection,
    wait_for_projection,
};

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use nostr::util::JsonUtil as _;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

/// kind:23194 — NIP-47 NWC request (client → wallet).
pub const KIND_NWC_REQUEST: u64 = 23194;
/// kind:23195 — NIP-47 NWC response (wallet → client).
pub const KIND_NWC_RESPONSE: u64 = 23195;
/// kind:9735 — NIP-57 zap receipt (LN provider → relays).
pub const KIND_ZAP_RECEIPT: u64 = 9735;

/// Find an OS-assigned free TCP port by binding `:0` and reading the port back.
///
/// There is an inherent TOCTOU window between releasing the listener and `nak`
/// re-binding it; for a single launcher on a developer/CI box this is benign,
/// and the launcher verifies the relay actually came up before returning.
fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);
    port
}

/// A running `nak serve` in-memory relay. Drops the child on `Drop`.
pub struct NakRelay {
    child: Child,
    ws_url: String,
}

impl NakRelay {
    /// Launch `nak serve` on a free port and block until it logs that it is
    /// listening. Returns `None` (so the caller can SKIP, not fake-pass) when
    /// the `nak` binary is absent or never reports readiness.
    pub fn spawn() -> Option<Self> {
        let port = free_port();
        let mut child = Command::new("nak")
            .arg("serve")
            .arg("--hostname")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .ok()?;

        let ws_url = format!("ws://127.0.0.1:{port}");

        // `nak serve` prints `relay running at ws://...` to stderr once it is
        // listening. Block on that line (with a ceiling) rather than sleeping
        // a fixed interval (D8).
        let stderr = child.stderr.take()?;
        let ready = Arc::new(AtomicBool::new(false));
        let ready_w = Arc::clone(&ready);
        let reader_handle = thread::spawn(move || {
            let mut lines = BufReader::new(stderr).lines();
            for line in lines.by_ref() {
                let Ok(line) = line else { break };
                if line.contains("relay running") || line.contains("running at") {
                    ready_w.store(true, Ordering::SeqCst);
                    break;
                }
            }
            // Keep draining so the child never blocks on a full stderr pipe.
            for _ in lines {}
        });

        // Bounded wait for readiness: the reader thread sets the flag the
        // moment the relay logs it is up. We block on the reader join with a
        // deadline by also probing a TCP connect — whichever proves liveness
        // first wins. No sleep-spin: we join the reader thread (it returns as
        // soon as it sees the line) under a wall-clock guard via a connect probe.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if ready.load(Ordering::SeqCst) {
                break;
            }
            // A successful TCP connect is also proof of liveness (the WS upgrade
            // happens later). This is an event probe, not a poll of mutable
            // state — it blocks in connect() and returns on the OS event.
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = reader_handle.join();
                return None;
            }
            // Yield to the OS scheduler between connect attempts. This is a
            // backoff on a connection-refused error, not a poll of in-process
            // state — bounded by the deadline above.
            thread::sleep(Duration::from_millis(25));
        }
        // Detach the stderr drainer — it exits when the child's stderr closes.
        drop(reader_handle);

        Some(Self { child, ws_url })
    }

    /// The `ws://127.0.0.1:<port>` URL the kernel and the fake wallet connect to.
    pub fn ws_url(&self) -> &str {
        &self.ws_url
    }
}

impl Drop for NakRelay {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

type RelaySocket = WebSocket<MaybeTlsStream<std::net::TcpStream>>;

fn open_ws(url: &str) -> Result<RelaySocket, String> {
    let (mut socket, _resp) = connect(url).map_err(|e| format!("connect {url}: {e}"))?;
    if let MaybeTlsStream::Plain(s) = socket.get_mut() {
        let _ = s.set_read_timeout(Some(Duration::from_millis(200)));
    }
    Ok(socket)
}

/// Outcome the fake wallet records for the one payment it handled.
#[derive(Clone, Debug, Default)]
pub struct WalletObservation {
    /// The decrypted bolt11 the kernel asked the wallet to pay.
    pub paid_bolt11: Option<String>,
    /// The kind:23194 request event id the wallet replied to.
    pub request_event_id: Option<String>,
}

/// A scripted NIP-47 wallet service bound to one `nak serve` relay.
///
/// Speaks the wallet half of NIP-47: subscribes to kind:23194 `#p
/// <wallet_pubkey>`, decrypts each `pay_invoice` request, and replies with a
/// kind:23195 success carrying a deterministic preimage. Runs on its own
/// thread; [`stop`] joins it and returns what it observed.
pub struct FakeNwcWallet {
    handle: JoinHandle<WalletObservation>,
    stop: Arc<AtomicBool>,
    /// Fires once, carrying the paid bolt11, the instant the wallet thread has
    /// answered a `pay_invoice` request. Lets the test block deterministically
    /// for the payment (D8) instead of polling a projection the FFI does not
    /// surface (`action_results` is kernel-internal, not a registered closure).
    paid_rx: std::sync::mpsc::Receiver<String>,
}

impl FakeNwcWallet {
    /// Start the wallet service.
    ///
    /// * `relay_url` — the `nak serve` URL (same relay the kernel uses).
    /// * `wallet_secret_hex` — the wallet service's secret key. Its pubkey is
    ///   the `wallet_pubkey_hex` half of the NWC URI handed to the kernel.
    /// * `client_secret_hex` — the NWC client secret (the other half of the
    ///   URI). The wallet derives the client pubkey from it to encrypt the
    ///   response and to filter `#p`-addressed requests.
    pub fn spawn(
        relay_url: &str,
        wallet_secret_hex: &str,
        client_secret_hex: &str,
    ) -> Result<Self, String> {
        let relay_url = relay_url.to_string();
        let wallet_secret_hex = wallet_secret_hex.to_string();
        let client_secret_hex = client_secret_hex.to_string();
        let client_pubkey_hex = nmp_nwc::crypto::client_pubkey_hex(&client_secret_hex)
            .map_err(|e| format!("derive client pubkey: {e:?}"))?;
        let wallet_pubkey_hex = nmp_nwc::crypto::client_pubkey_hex(&wallet_secret_hex)
            .map_err(|e| format!("derive wallet pubkey: {e:?}"))?;

        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let (paid_tx, paid_rx) = std::sync::mpsc::channel::<String>();

        let mut socket = open_ws(&relay_url)?;
        // REQ for pay_invoice requests addressed to this wallet.
        let sub_id = "fake-wallet-sub";
        let req = serde_json::json!([
            "REQ",
            sub_id,
            { "kinds": [KIND_NWC_REQUEST], "#p": [wallet_pubkey_hex] }
        ]);
        socket
            .send(Message::Text(req.to_string()))
            .map_err(|e| format!("send REQ: {e}"))?;

        let handle = thread::spawn(move || {
            let mut obs = WalletObservation::default();
            let deadline = Instant::now() + Duration::from_secs(20);
            let mut paid = false;
            while !stop_thread.load(Ordering::SeqCst) && Instant::now() < deadline {
                // Blocking read with the socket's 200ms read timeout. A timeout
                // surfaces as a WouldBlock/io error we treat as "keep waiting"
                // until the stop flag or deadline — this is an event wait on the
                // socket, not a poll of in-process mutable state (D8).
                let msg = match socket.read() {
                    Ok(m) => m,
                    Err(tungstenite::Error::Io(e))
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        continue;
                    }
                    Err(_) => break,
                };
                let text = match msg {
                    Message::Text(t) => t,
                    Message::Ping(p) => {
                        let _ = socket.send(Message::Pong(p));
                        continue;
                    }
                    Message::Close(_) => break,
                    _ => continue,
                };
                // Decode any NWC request (get_info / get_balance / pay_invoice).
                // The runtime sends get_info + get_balance on connect; the
                // wallet status only flips to "ready" once one of those is
                // answered, so the harness MUST answer them before the
                // pay_invoice path is allowed to proceed.
                let Some(req) =
                    decode_nwc_request(&text, &wallet_secret_hex, &client_pubkey_hex)
                else {
                    continue;
                };
                let result_body = match req.method.as_str() {
                    "get_info" => Some(serde_json::json!({
                        "result_type": "get_info",
                        "result": {
                            "alias": "fake-nwc-wallet",
                            "methods": ["get_info", "get_balance", "pay_invoice"],
                        }
                    })),
                    "get_balance" => Some(serde_json::json!({
                        "result_type": "get_balance",
                        "result": { "balance": 100_000_000u64 }
                    })),
                    "pay_invoice" => {
                        obs.request_event_id = Some(req.request_id.clone());
                        obs.paid_bolt11 = req.bolt11.clone();
                        paid = true;
                        let _ = paid_tx.send(req.bolt11.clone().unwrap_or_default());
                        Some(serde_json::json!({
                            "result_type": "pay_invoice",
                            "result": { "preimage": "00".repeat(32) }
                        }))
                    }
                    _ => None,
                };
                if let Some(body) = result_body {
                    let response_event = build_response_event(
                        &wallet_secret_hex,
                        &client_pubkey_hex,
                        &req.request_id,
                        &body,
                    );
                    let event_frame =
                        serde_json::json!(["EVENT", response_event]).to_string();
                    let _ = socket.send(Message::Text(event_frame));
                    let _ = socket.flush();
                }
                // Exit once the payment has been answered AND a brief drain
                // window has let the EVENT flush to the relay.
                if paid {
                    let flush_deadline = Instant::now() + Duration::from_millis(500);
                    while Instant::now() < flush_deadline
                        && !stop_thread.load(Ordering::SeqCst)
                    {
                        match socket.read() {
                            Ok(_) => {}
                            Err(_) => break,
                        }
                    }
                    break;
                }
            }
            let _ = socket.close(None);
            obs
        });

        Ok(Self {
            handle,
            stop,
            paid_rx,
        })
    }

    /// Block (up to `timeout`) until the wallet has answered a `pay_invoice`
    /// request, returning the bolt11 it paid. Returns `None` on timeout. This
    /// is an event wait on the wallet thread's channel — no polling (D8).
    pub fn wait_paid(&self, timeout: Duration) -> Option<String> {
        self.paid_rx.recv_timeout(timeout).ok()
    }

    /// Signal the wallet thread to stop and return what it observed.
    pub fn stop(self) -> WalletObservation {
        self.stop.store(true, Ordering::SeqCst);
        self.handle.join().unwrap_or_default()
    }
}

/// A decoded NIP-47 kind:23194 request the fake wallet must answer.
struct NwcRequest {
    request_id: String,
    method: String,
    /// Present only for `pay_invoice` (`params.invoice`).
    bolt11: Option<String>,
}

/// Decode a relay `["EVENT", sub, {kind:23194 …}]` frame into the request id,
/// method, and (for `pay_invoice`) the bolt11. Returns `None` for any frame
/// that isn't a decryptable kind:23194 NWC request.
fn decode_nwc_request(
    relay_text: &str,
    wallet_secret_hex: &str,
    client_pubkey_hex: &str,
) -> Option<NwcRequest> {
    let outer: serde_json::Value = serde_json::from_str(relay_text).ok()?;
    let arr = outer.as_array()?;
    if arr.first()?.as_str()? != "EVENT" || arr.len() < 3 {
        return None;
    }
    let event = arr.get(2)?;
    if event.get("kind")?.as_u64()? != KIND_NWC_REQUEST {
        return None;
    }
    let request_id = event.get("id")?.as_str()?.to_string();
    let content = event.get("content")?.as_str()?;
    // The wallet decrypts the request: its own secret + the client's pubkey.
    let plaintext = nmp_nwc::crypto::decrypt(wallet_secret_hex, client_pubkey_hex, content).ok()?;
    let payload: serde_json::Value = serde_json::from_str(&plaintext).ok()?;
    let method = payload.get("method")?.as_str()?.to_string();
    let bolt11 = payload
        .get("params")
        .and_then(|p| p.get("invoice"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    Some(NwcRequest {
        request_id,
        method,
        bolt11,
    })
}

/// Build a signed kind:23195 response event whose content is `result_body`
/// encrypted to the client pubkey, tagged `["e", <request_id>]` +
/// `["p", <client_pubkey>]` — the exact shape `try_decode_response_for_request`
/// matches against.
fn build_response_event(
    wallet_secret_hex: &str,
    client_pubkey_hex: &str,
    request_id: &str,
    result_body: &serde_json::Value,
) -> serde_json::Value {
    let content = nmp_nwc::crypto::encrypt(
        wallet_secret_hex,
        client_pubkey_hex,
        &result_body.to_string(),
    )
    .expect("encrypt NWC response");

    let wallet_keys = Keys::parse(wallet_secret_hex).expect("wallet keys");
    let client_pk = nostr::PublicKey::from_hex(client_pubkey_hex).expect("client pubkey");
    let event = EventBuilder::new(Kind::from_u16(KIND_NWC_RESPONSE as u16), content)
        .tags(vec![
            Tag::event(nostr::EventId::from_hex(request_id).expect("request id")),
            Tag::public_key(client_pk),
        ])
        .sign_with_keys(&wallet_keys)
        .expect("sign kind:23195");
    serde_json::from_str(&event.as_json()).expect("event json")
}

/// Build a real Schnorr-signed kind:9735 zap receipt for `target_event_id`,
/// signed by `provider_keys` (the LN provider's nostrPubkey identity).
///
/// NIP-57 Appendix E: the receipt carries `["p", recipient]`, `["e",
/// target]`, `["bolt11", …]`, and `["description", <kind:9734 json>]`. The
/// visible-card relation counts key zap totals off the `["e", target]` tag
/// and decodes the amount from the bolt11 HRP.
pub fn signed_zap_receipt_json(
    provider_keys: &Keys,
    recipient_pubkey_hex: &str,
    target_event_id_hex: &str,
    bolt11: &str,
    zap_request_json: &str,
) -> String {
    let recipient = nostr::PublicKey::from_hex(recipient_pubkey_hex).expect("recipient pubkey");
    let target = nostr::EventId::from_hex(target_event_id_hex).expect("target event id");
    let event = EventBuilder::new(Kind::from_u16(KIND_ZAP_RECEIPT as u16), String::new())
        .tags(vec![
            Tag::public_key(recipient),
            Tag::event(target),
            Tag::parse(["bolt11", bolt11]).expect("bolt11 tag"),
            Tag::parse(["description", zap_request_json]).expect("description tag"),
        ])
        .custom_created_at(Timestamp::from(1_700_000_000))
        .sign_with_keys(provider_keys)
        .expect("sign kind:9735");
    event.as_json()
}

/// Publish a pre-built signed event JSON to the relay over a one-shot socket.
/// Blocks until the relay returns its `OK` (or the deadline elapses).
pub fn publish_event(relay_url: &str, event_json: &str) -> Result<(), String> {
    let mut socket = open_ws(relay_url)?;
    let event: serde_json::Value =
        serde_json::from_str(event_json).map_err(|e| format!("parse event: {e}"))?;
    let frame = serde_json::json!(["EVENT", event]).to_string();
    socket
        .send(Message::Text(frame))
        .map_err(|e| format!("send EVENT: {e}"))?;
    let _ = socket.flush();
    // Block for the relay's OK acknowledgement (event-driven, deadline-bounded).
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match socket.read() {
            Ok(Message::Text(t)) if t.contains("\"OK\"") => {
                let _ = socket.close(None);
                return Ok(());
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => return Err(format!("read OK: {e}")),
        }
    }
    Err("relay did not acknowledge EVENT within deadline".to_string())
}
