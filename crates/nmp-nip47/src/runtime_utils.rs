//! Private utilities shared by the NWC runtime — extracted to keep `runtime.rs`
//! within the 500-LOC ceiling.

use nostr::nips::nip19::ToBech32;
use nostr::PublicKey;

/// Serialize a JSON value to a string for the outbound wire queue.
///
/// V-63: replaces the prior `serde_json::to_string(...).unwrap_or_default()`
/// call sites. Returns `Err` on the rare serialization failure so callers can
/// surface an error rather than pushing an empty `""` frame.
pub(super) fn encode_frame(value: &serde_json::Value) -> Result<String, serde_json::Error> {
    serde_json::to_string(value)
}

pub(super) fn pubkey_to_npub(hex: &str) -> Result<String, String> {
    PublicKey::from_hex(hex)
        .map_err(|e| format!("{e}"))?
        .to_bech32()
        .map_err(|e| format!("{e}"))
}
