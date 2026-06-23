//! Small kernel-module constants, NIP-42 AUTH credentials, and the hex-decode helper.
//!
//! Extracted from `kernel/mod.rs` to honour the 500-LOC ceiling.

use super::AuthSignerFn;

/// Decode a 64-char hex pubkey into `[u8; 32]`. Returns `None` on malformed input (D6).
pub(crate) fn hex_to_pubkey_bytes(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let hi = (chunk[0] as char).to_digit(16)? as u8;
        let lo = (chunk[1] as char).to_digit(16)? as u8;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

/// Per-pubkey claim consumer-id retention cap (T114b — D8 guard against unbounded growth).
pub(crate) const MAX_CLAIMS_PER_PUBKEY: usize = 256;

/// Per-`primary_id` event-claim consumer-id retention cap (mirrors `MAX_CLAIMS_PER_PUBKEY`).
pub(crate) const MAX_EVENT_CLAIMS_PER_KEY: usize = 256;

/// F-TTL inflight REQ guard duration (unix milliseconds, 1 hour).
pub(crate) const INFLIGHT_GUARD_MS: u64 = 3_600_000;

/// Per-relay-role NIP-42 credentials used by the AUTH handshake.
pub(crate) struct RelayAuthCredentials {
    pub(crate) signer: AuthSignerFn,
    pub(crate) pubkey_hex: String,
}

/// V-58 — kernel-side backoff hint for a relay URL.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BackoffHint {
    /// Relay issued `CLOSED ["rate-limited: …"]` — use `RELAY_RECONNECT_DELAY_RATE_LIMITED`.
    RateLimited,
}
