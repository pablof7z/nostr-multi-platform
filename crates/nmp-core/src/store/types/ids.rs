//! Core identifier type aliases for `EventStore`.
//!
//! D0: belongs to `nmp-core::store`; no cross-module dependency.

// ─── Type aliases ────────────────────────────────────────────────────────────

pub type EventId = [u8; 32];
pub type PubKey = [u8; 32];
pub type RelayUrl = String;

// ─── Hex utilities ───────────────────────────────────────────────────────────

pub(super) fn hex_to_bytes32(s: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    if s.len() != 64 {
        return out;
    }
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        if i >= 32 {
            break;
        }
        if let (Some(&hi), Some(&lo)) = (chunk.first(), chunk.get(1)) {
            out[i] = (hex_nibble(hi) << 4) | hex_nibble(lo);
        }
    }
    out
}

pub(super) fn hex_nibble(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}
