//! NIP-46 RPC helpers: request-id generation, connect-params, response
//! decoding, and the shared kind:24133 wire-frame builder.
//!
//! [`build_event_frame`] is the single authoritative NIP-44-encrypt +
//! kind:24133 wrap + `["EVENT",..]` serialisation. Both the handshake
//! helpers in [`crate::bunker`] / [`crate::nostrconnect`] and the broker's
//! steady-state `send_rpc` drive through this one function, eliminating the
//! prior duplicate implementations.

use std::time::{SystemTime, UNIX_EPOCH};

use nostr::nips::nip44;
use nostr::{EventBuilder, Keys, Kind, PublicKey, Tag, Timestamp};
use serde_json::{json, Value};

// ─── RpcBuildError ───────────────────────────────────────────────────────────

/// Errors returned by [`build_event_frame`].
#[derive(Debug)]
pub enum RpcBuildError {
    /// NIP-44 encryption failed.
    Encrypt(String),
    /// `["p", <hex>]` tag construction failed.
    TagParse(String),
    /// Event signing failed.
    Sign(String),
    /// JSON serialisation of the signed event failed.
    Serialize(String),
}

impl std::fmt::Display for RpcBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Encrypt(s) => write!(f, "nip44 encrypt: {s}"),
            Self::TagParse(s) => write!(f, "tag parse: {s}"),
            Self::Sign(s) => write!(f, "sign event: {s}"),
            Self::Serialize(s) => write!(f, "serialize event: {s}"),
        }
    }
}

impl std::error::Error for RpcBuildError {}

// ─── build_event_frame ───────────────────────────────────────────────────────

/// Encrypt `plaintext` with NIP-44 V2 (sender = `local_keys`, recipient =
/// `remote`), wrap the ciphertext in a kind:24133 event tagged `["p",
/// remote_hex]`, sign it with `local_keys`, and serialise to the NIP-01
/// `["EVENT", <event>]` wire frame.
///
/// This is the single authoritative frame builder for all NIP-46 outbound
/// messages. Callers form the JSON-RPC envelope (`{id, method, params}`) and
/// pass it here as `plaintext`.
///
/// ## Wire shape (verified identical to the prior two implementations)
///
/// - `kind = 24133`
/// - single `["p", <remote_pubkey_hex>]` tag
/// - `created_at = floor(now_unix_secs)` (via `custom_created_at`)
/// - NIP-44 V2 encryption
pub fn build_event_frame(
    local_keys: &Keys,
    remote: PublicKey,
    plaintext: &str,
) -> Result<String, RpcBuildError> {
    let ciphertext = nip44::encrypt(
        local_keys.secret_key(),
        &remote,
        plaintext.as_bytes(),
        nip44::Version::V2,
    )
    .map_err(|e| RpcBuildError::Encrypt(e.to_string()))?;

    let event = EventBuilder::new(Kind::from_u16(24133), ciphertext)
        .tags(vec![Tag::parse(["p", &remote.to_hex()])
            .map_err(|e| RpcBuildError::TagParse(e.to_string()))?])
        .custom_created_at(Timestamp::from(now_secs()))
        .sign_with_keys(local_keys)
        .map_err(|e| RpcBuildError::Sign(e.to_string()))?;

    let serialized =
        serde_json::to_string(&event).map_err(|e| RpcBuildError::Serialize(e.to_string()))?;

    Ok(format!(r#"["EVENT",{serialized}]"#))
}

// ─── build_connect_params ────────────────────────────────────────────────────

/// Build the `connect` params list.
///
/// NIP-46 spec accepts either `[remote, secret]` or `[remote, secret, perms]`.
/// We always send the 3-tuple, with empty strings filling absent fields —
/// this is what most modern bunkers expect.
pub(crate) fn build_connect_params(
    remote: PublicKey,
    secret: Option<&str>,
    perms: Option<&str>,
) -> Value {
    json!([remote.to_hex(), secret.unwrap_or(""), perms.unwrap_or(""),])
}

// ─── new_request_id ──────────────────────────────────────────────────────────

/// Generate a request id (11-byte lowercase hex, mirroring the
/// `nmp-signers::mapper::generate_request_id` shape).
pub(crate) fn new_request_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering as AOrd};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, AOrd::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    format!(
        "{:011x}",
        (n.wrapping_mul(0x9E37_79B9) ^ nanos) & 0xFFFF_FFFF_FFFF
    )
}

// ─── decode_inbound_response ─────────────────────────────────────────────────

/// Steady-state inbound dispatcher: parse a raw kind:24133 event, decrypt the
/// content with NIP-44, and return the decrypted plaintext for the signer's
/// `deliver_response`.
///
/// Returns `None` if the event is malformed or addressed to a different key
/// (D6 — silent drop for non-fatal errors).
#[must_use]
pub fn decode_inbound_response(
    event: &Value,
    local_keys: &Keys,
    remote_pubkey: PublicKey,
) -> Option<String> {
    let ciphertext = event.get("content").and_then(|v| v.as_str())?;
    let event_pubkey = event.get("pubkey").and_then(|v| v.as_str())?;
    if event_pubkey.to_ascii_lowercase() != remote_pubkey.to_hex() {
        return None;
    }
    nip44::decrypt(
        local_keys.secret_key(),
        &remote_pubkey,
        ciphertext.as_bytes(),
    )
    .ok()
}

// ─── helpers ─────────────────────────────────────────────────────────────────

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
