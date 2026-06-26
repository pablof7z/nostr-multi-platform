//! Transport-agnostic NIP-46 handshake state machine (the "reducer").
//!
//! [`SessionState`] implements the core protocol logic as a pure function:
//! each entry point takes the current state + an input event and returns a
//! new state + a list of [`Effect`]s to execute. The caller drives it from
//! whatever thread/executor it owns; the reducer has no I/O, no threads,
//! no `SystemTime`, and no `crossbeam` dependency — making it correct in
//! both native and wasm runtimes.
//!
//! ## State machine phases
//!
//! **Bunker flow** (`bunker://` URI):
//! - `BunkerWaitConnectAck` → wait for the bunker's `connect` response.
//! - `WaitGpk` → wait for the `get_public_key` response.
//! - `Done` → terminal; further inputs are silently ignored.
//!
//! **NostrConnect flow** (`nostrconnect://` URI):
//! - `NostrConnectWaitConnect` → wait for the signer's initial `connect` frame.
//! - `WaitGpk` → wait for the `get_public_key` response (shared with bunker).
//! - `Done` → terminal.
//!
//! ## Error handling (D6 — silent skip for non-fatal)
//!
//! Every stray event (wrong pubkey, undecryptable, wrong id, malformed JSON)
//! returns an empty `Vec<Effect>` with no state change. Only events that
//! semantically match the expected response advance the state.

use nostr::nips::nip44;
use nostr::{Keys, PublicKey};
use serde_json::{json, Value};

use crate::effect::{Effect, SignerReady};
use crate::error::HandshakeError;
use crate::progress_codes;
use crate::rpc::{build_event_frame_at, RpcBuildError};

/// Per-step deadline in seconds — the same 60s budget as STEP_TIMEOUT in
/// the former blocking implementation.
const STEP_TIMEOUT_SECS: u64 = 60;

// ─── Phase ───────────────────────────────────────────────────────────────────

pub(crate) enum Phase {
    /// bunker://: waiting for the bunker's `connect` response.
    BunkerWaitConnectAck {
        connect_id: String,
        remote_pubkey: PublicKey,
    },
    /// Both flows: waiting for the `get_public_key` response.
    WaitGpk {
        gpk_id: String,
        remote_pubkey: PublicKey,
    },
    /// nostrconnect://: waiting for the signer's initial `connect` frame.
    NostrConnectWaitConnect {
        expected_secret: String,
    },
    /// Terminal — all further inputs are no-ops.
    Done,
}

// ─── SessionState ─────────────────────────────────────────────────────────────

/// Handshake state for a single NIP-46 session. Constructed by
/// [`crate::bunker::start_bunker`] or [`crate::nostrconnect::start_nostrconnect`]
/// and then driven by the caller via [`Self::on_relay_event`] / [`Self::tick`].
pub struct SessionState {
    phase: Phase,
    pub(crate) local_keys: Keys,
    relay_url: String,
    sub_id: String,
    perms: Option<String>,
    req_counter: u64,
    deadline_at: u64,
}

impl SessionState {
    // ─── Constructor (crate-internal) ─────────────────────────────────────

    pub(crate) fn new(
        phase: Phase,
        local_keys: Keys,
        relay_url: String,
        sub_id: String,
        perms: Option<String>,
        req_counter: u64,
        deadline_at: u64,
    ) -> Self {
        Self {
            phase,
            local_keys,
            relay_url,
            sub_id,
            perms,
            req_counter,
            deadline_at,
        }
    }

    // ─── Public accessors ─────────────────────────────────────────────────

    /// The Unix-second deadline for the current step. The caller should arm a
    /// timer and call [`Self::tick`] when this timestamp is reached or passed.
    pub fn deadline_at(&self) -> u64 {
        self.deadline_at
    }

    /// (Re-)arm the current step deadline to `now + STEP_TIMEOUT_SECS`.
    ///
    /// The driver calls this once the relay is connected AND the inbound REQ
    /// subscription is installed, so the 60s step budget starts counting only
    /// from the point the session can actually receive responses — not from
    /// `start_bunker` / `start_nostrconnect`, which run before the (up to 10s)
    /// relay dial. This matches the prior blocking implementation, where the
    /// deadline was `Instant::now() + timeout`, set inside `await_response`
    /// AFTER the worker's connect + subscribe completed. Terminal (`Done`)
    /// sessions are left untouched.
    pub fn arm_deadline(&mut self, now: u64) {
        if !matches!(self.phase, Phase::Done) {
            self.deadline_at = now + STEP_TIMEOUT_SECS;
        }
    }

    // ─── Internal helpers ─────────────────────────────────────────────────

    fn next_req_id(&mut self) -> String {
        let n = self.req_counter;
        self.req_counter += 1;
        format!("{:011x}", n)
    }

    /// Human-readable label for the current wait step (used in timeout messages).
    fn phase_label(&self) -> &'static str {
        match &self.phase {
            Phase::BunkerWaitConnectAck { .. } => "connect",
            Phase::WaitGpk { .. } => "get_public_key",
            Phase::NostrConnectWaitConnect { .. } => "connect frame from signer",
            Phase::Done => "done",
        }
    }

    // ─── Public reducer entry points ──────────────────────────────────────

    /// Drive the step-deadline timer. Returns an `Error` effect if the
    /// deadline has elapsed; returns an empty `Vec` if there is still time
    /// remaining.
    pub fn tick(&mut self, now: u64) -> Vec<Effect> {
        if now < self.deadline_at {
            return Vec::new();
        }
        match &self.phase {
            Phase::Done => Vec::new(),
            _ => {
                let label = self.phase_label();
                vec![Effect::Error {
                    // Byte-identical to the old wait.rs Timeout message:
                    //   format!("no response to {method_label} within {timeout:?}")
                    // where timeout = Duration::from_secs(60) → Debug "60s".
                    error: HandshakeError::Timeout(format!(
                        "no response to {label} within 60s"
                    )),
                }]
            }
        }
    }

    /// Drive the state machine from a raw relay text frame.
    ///
    /// - `["EVENT", sub_id, event_json]` → delegates to [`Self::on_relay_event`].
    /// - `["EOSE", sub_id]` → arms the per-step deadline (Guardrail 2): the
    ///   relay is confirming it has sent all stored events, so any response to
    ///   our in-flight RPC will arrive now.  The handshake-start deadline from
    ///   `start_bunker`/`start_nostrconnect` remains as a fallback floor for
    ///   relays that never send EOSE.
    /// - All other frames and frames for other subscriptions are silently ignored.
    pub fn on_relay_text(&mut self, text: &str, now: u64) -> Vec<Effect> {
        let v: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };
        let arr = match v.as_array() {
            Some(a) if !a.is_empty() => a,
            _ => return Vec::new(),
        };
        let msg_type = arr[0].as_str().unwrap_or("");

        // EOSE for our subscription: arm the per-step deadline so the 60 s
        // budget counts from the point the relay is ready to deliver responses.
        if msg_type == "EOSE" {
            if arr.len() >= 2 && arr[1].as_str() == Some(self.sub_id.as_str()) {
                self.arm_deadline(now);
            }
            return Vec::new();
        }

        // Only process EVENT frames.
        if msg_type != "EVENT" {
            return Vec::new();
        }
        if arr.len() < 3 {
            return Vec::new();
        }
        // Only handle frames for THIS session's subscription. A relay multiplexes
        // many REQs over one socket; frames for other sub ids belong to other
        // consumers and must be ignored (matters for the step-3 browser caller
        // that forwards raw socket text straight into `on_relay_text`).
        if arr[1].as_str() != Some(self.sub_id.as_str()) {
            return Vec::new();
        }
        let event = arr[2].clone();
        self.on_relay_event(&event, now)
    }

    /// Re-emit the subscription frame after a relay reconnect so the inbound
    /// REQ survives a transient socket drop (V-14 replay invariant).
    pub fn on_relay_connected(&mut self, _is_reconnect: bool, _now: u64) -> Vec<Effect> {
        let pubkey_hex = self.local_keys.public_key().to_hex();
        // `build_req_frame` needs `now`; use 0 as a placeholder — the broker's
        // relay client calls `subscribe()` immediately on reconnect so the
        // `since` filter is less critical for replay frames. The primary path
        // through `start_bunker`/`start_nostrconnect` uses the real wall-clock.
        let req_frame = crate::rpc::build_req_frame(&self.sub_id, &pubkey_hex, 0);
        let relay_url = self.relay_url.clone();
        vec![Effect::Subscribe { relay_url, frame: req_frame }]
    }

    /// Drive the state machine from a pre-parsed kind:24133 event JSON value.
    /// Stray events (wrong pubkey, undecryptable, wrong request id, malformed)
    /// return an empty `Vec` with no state change (D6 — silent skip).
    pub fn on_relay_event(&mut self, event: &Value, now: u64) -> Vec<Effect> {
        match &self.phase {
            Phase::BunkerWaitConnectAck { .. } => self.handle_bunker_connect_ack(event, now),
            Phase::WaitGpk { .. } => self.handle_gpk_response(event),
            Phase::NostrConnectWaitConnect { .. } => self.handle_nc_connect(event, now),
            Phase::Done => Vec::new(),
        }
    }

    // ─── Phase handlers ───────────────────────────────────────────────────

    fn handle_bunker_connect_ack(&mut self, event: &Value, now: u64) -> Vec<Effect> {
        let (connect_id, remote_pubkey) = match &self.phase {
            Phase::BunkerWaitConnectAck { connect_id, remote_pubkey } => {
                (connect_id.clone(), *remote_pubkey)
            }
            _ => return Vec::new(),
        };

        // Stray-event checks (D6 — wrong pubkey or undecryptable → skip silently).
        let event_pubkey = event.get("pubkey").and_then(|v| v.as_str()).unwrap_or("");
        if event_pubkey.to_ascii_lowercase() != remote_pubkey.to_hex() {
            return Vec::new();
        }
        let Some(ciphertext) = event.get("content").and_then(|v| v.as_str()) else {
            return Vec::new();
        };
        let Ok(plaintext) =
            nip44::decrypt(self.local_keys.secret_key(), &remote_pubkey, ciphertext.as_bytes())
        else {
            return Vec::new();
        };
        let Ok(rpc) = serde_json::from_str::<Value>(&plaintext) else {
            return Vec::new();
        };
        if rpc.get("id").and_then(|v| v.as_str()) != Some(&connect_id) {
            return Vec::new(); // wrong id — skip
        }

        // Bunker error response.
        if let Some(err) = rpc.get("error") {
            if !err.is_null() {
                let msg = err.as_str().map_or_else(|| err.to_string(), str::to_string);
                return vec![Effect::Error { error: HandshakeError::BunkerError(msg) }];
            }
        }

        // `connect` must return a string result (matches old await_response behaviour).
        if rpc.get("result").and_then(|v| v.as_str()).is_none() {
            return vec![Effect::Error {
                error: HandshakeError::Protocol(
                    "connect response missing string result".to_string(),
                ),
            }];
        }

        // Success — advance to WaitGpk.
        let gpk_id = self.next_req_id();
        let gpk_envelope = json!({
            "id": &gpk_id,
            "method": "get_public_key",
            "params": Value::Array(Vec::new()),
        })
        .to_string();

        match build_event_frame_at(&self.local_keys, remote_pubkey, &gpk_envelope, now) {
            Ok(frame) => {
                let relay_url = self.relay_url.clone();
                self.deadline_at = now + STEP_TIMEOUT_SECS;
                self.phase = Phase::WaitGpk { gpk_id, remote_pubkey };
                vec![
                    Effect::Progress {
                        stage: "awaiting_pubkey".to_string(),
                        code: Some(progress_codes::AWAITING_BUNKER_APPROVAL.to_string()),
                        detail: Some("Awaiting bunker approval".to_string()),
                    },
                    Effect::SendFrame { relay_url, text: frame },
                ]
            }
            Err(e) => vec![Effect::Error {
                error: HandshakeError::Protocol(e.to_string()),
            }],
        }
    }

    fn handle_gpk_response(&mut self, event: &Value) -> Vec<Effect> {
        let (gpk_id, remote_pubkey) = match &self.phase {
            Phase::WaitGpk { gpk_id, remote_pubkey } => (gpk_id.clone(), *remote_pubkey),
            _ => return Vec::new(),
        };

        let event_pubkey = event.get("pubkey").and_then(|v| v.as_str()).unwrap_or("");
        if event_pubkey.to_ascii_lowercase() != remote_pubkey.to_hex() {
            return Vec::new();
        }
        let Some(ciphertext) = event.get("content").and_then(|v| v.as_str()) else {
            return Vec::new();
        };
        let Ok(plaintext) =
            nip44::decrypt(self.local_keys.secret_key(), &remote_pubkey, ciphertext.as_bytes())
        else {
            return Vec::new();
        };
        let Ok(rpc) = serde_json::from_str::<Value>(&plaintext) else {
            return Vec::new();
        };
        if rpc.get("id").and_then(|v| v.as_str()) != Some(&gpk_id) {
            return Vec::new();
        }

        if let Some(err) = rpc.get("error") {
            if !err.is_null() {
                let msg = err.as_str().map_or_else(|| err.to_string(), str::to_string);
                return vec![Effect::Error { error: HandshakeError::BunkerError(msg) }];
            }
        }

        let result = match rpc.get("result").and_then(|v| v.as_str()) {
            Some(r) => r,
            None => {
                return vec![Effect::Error {
                    error: HandshakeError::Protocol(
                        "get_public_key response missing string result".to_string(),
                    ),
                }]
            }
        };

        let user_pubkey_hex = result.trim().to_ascii_lowercase();
        if user_pubkey_hex.len() != 64 || !user_pubkey_hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return vec![Effect::Error {
                error: HandshakeError::Protocol(format!(
                    "get_public_key returned non-hex: {:?}",
                    result.trim()
                )),
            }];
        }

        let remote_signer_pubkey_hex = remote_pubkey.to_hex();
        let granted_perms = self.perms.clone();
        self.phase = Phase::Done;

        vec![Effect::SignerReady(SignerReady {
            user_pubkey_hex,
            remote_signer_pubkey_hex,
            granted_perms,
        })]
    }

    fn handle_nc_connect(&mut self, event: &Value, now: u64) -> Vec<Effect> {
        let expected_secret = match &self.phase {
            Phase::NostrConnectWaitConnect { expected_secret } => expected_secret.clone(),
            _ => return Vec::new(),
        };

        // Extract signer pubkey from event.pubkey.
        let signer_pubkey_hex = match event.get("pubkey").and_then(|v| v.as_str()) {
            Some(pk) => pk.to_ascii_lowercase(),
            None => return Vec::new(),
        };
        if signer_pubkey_hex.len() != 64
            || !signer_pubkey_hex.chars().all(|c| c.is_ascii_hexdigit())
        {
            return Vec::new();
        }
        let Ok(signer_pk) = PublicKey::from_hex(&signer_pubkey_hex) else {
            return Vec::new();
        };

        let Some(ciphertext) = event.get("content").and_then(|v| v.as_str()) else {
            return Vec::new();
        };
        let Ok(plaintext) =
            nip44::decrypt(self.local_keys.secret_key(), &signer_pk, ciphertext.as_bytes())
        else {
            return Vec::new(); // not for us or malformed — skip
        };
        let rpc: Value = match serde_json::from_str(&plaintext) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };
        if rpc.get("method").and_then(|v| v.as_str()) != Some("connect") {
            return Vec::new();
        }

        let connect_id = match rpc.get("id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => return Vec::new(),
        };

        let Some(params) = rpc.get("params").and_then(|v| v.as_array()) else {
            return Vec::new();
        };

        let received_secret = params.get(1).and_then(|v| v.as_str()).unwrap_or("");
        if received_secret != expected_secret {
            // Wrong secret — reject with a definitive error (byte-identical to the
            // old `await_nostrconnect_connect` error text).
            return vec![Effect::Error {
                error: HandshakeError::BunkerError(format!(
                    "secret mismatch: expected {:?}, got {:?}",
                    expected_secret, received_secret
                )),
            }];
        }

        // Build ACK response (byte-identical error strings to the old
        // `map_ack_build_error` helper in nostrconnect.rs).
        let ack_body = json!({ "id": connect_id, "result": "ack" }).to_string();
        let ack_frame = match build_event_frame_at(&self.local_keys, signer_pk, &ack_body, now) {
            Ok(f) => f,
            Err(e) => {
                return vec![Effect::Error {
                    error: HandshakeError::Protocol(map_ack_build_error(&e)),
                }]
            }
        };

        // Build get_public_key.
        let gpk_id = self.next_req_id();
        let gpk_envelope = json!({
            "id": &gpk_id,
            "method": "get_public_key",
            "params": Value::Array(Vec::new()),
        })
        .to_string();
        let gpk_frame = match build_event_frame_at(&self.local_keys, signer_pk, &gpk_envelope, now)
        {
            Ok(f) => f,
            Err(e) => {
                return vec![Effect::Error {
                    error: HandshakeError::Protocol(e.to_string()),
                }]
            }
        };

        let relay_url = self.relay_url.clone();
        self.deadline_at = now + STEP_TIMEOUT_SECS;
        self.phase = Phase::WaitGpk { gpk_id, remote_pubkey: signer_pk };

        vec![
            Effect::SendFrame { relay_url: relay_url.clone(), text: ack_frame },
            Effect::Progress {
                stage: "awaiting_pubkey".to_string(),
                code: Some(progress_codes::NOSTRCONNECT_AWAITING_CONFIRMATION.to_string()),
                detail: Some("Awaiting user confirmation in signer app".to_string()),
            },
            Effect::SendFrame { relay_url, text: gpk_frame },
        ]
    }
}

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Map a [`RpcBuildError`] from the shared frame builder onto the ACK-specific
/// error strings the pre-extraction inline code produced. The `TagParse`
/// variant already matches the shared "tag parse: …" wording verbatim.
/// Byte-identical to the former `map_ack_build_error` in nostrconnect.rs.
fn map_ack_build_error(e: &RpcBuildError) -> String {
    match e {
        RpcBuildError::Encrypt(s) => format!("nip44 encrypt ack: {s}"),
        RpcBuildError::TagParse(s) => format!("tag parse: {s}"),
        RpcBuildError::Sign(s) => format!("sign ack event: {s}"),
        RpcBuildError::Serialize(s) => format!("serialize ack: {s}"),
    }
}

