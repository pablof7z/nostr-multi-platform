//! Core identifier type aliases for `EventStore`.
//!
//! D0: belongs to `nmp-store`; no cross-module dependency.

// ─── Type aliases ────────────────────────────────────────────────────────────

pub type EventId = [u8; 32];
pub type PubKey = [u8; 32];

/// A `wss://`/`ws://` URL for a relay, in plain (non-canonicalized) string form.
///
/// Transparent `String` alias (grep-able, swappable). The same alias lives in
/// `nmp_core::relay::RelayUrl` and `nmp_planner::RelayUrl`; the three are
/// definitionally identical (`pub type RelayUrl = String`) so a value produced
/// in one crate flows into the others without conversion.
pub type RelayUrl = String;

// ─── Hex utilities ───────────────────────────────────────────────────────────

/// Decode a 64-character lowercase/uppercase hex string to 32 bytes.
///
/// Returns `None` if `s.len() != 64` or any character is not a valid hex digit.
/// Callers must handle `None` explicitly — there is no silent all-zeros fallback.
pub(super) fn hex_to_bytes32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        if i >= 32 {
            break;
        }
        let (hi, lo) = match (chunk.first(), chunk.get(1)) {
            (Some(&h), Some(&l)) => (hex_nibble(h)?, hex_nibble(l)?),
            _ => return None,
        };
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

/// Decode a 64-character lowercase/uppercase hex string to an `EventId`.
///
/// Alias of `hex_to_bytes32` with a domain-typed return, exposed at
/// `pub(crate)` so `mem::list_seen_on` can convert stored hex ids back to
/// typed `EventId` values without re-implementing the decode.
///
/// Returns `None` for invalid hex strings — callers in `list_seen_on` skip
/// malformed entries (stored hex ids are always valid after `is_structurally_valid`).
pub(crate) fn hex_to_event_id(s: &str) -> Option<EventId> {
    hex_to_bytes32(s)
}

/// Decode a single hex nibble. Returns `None` for non-hex bytes.
pub(super) fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::hex_to_bytes32;

    #[test]
    fn hex_to_bytes32_valid_lowercase() {
        let s = "a".repeat(64);
        assert!(
            hex_to_bytes32(&s).is_some(),
            "64 valid lowercase hex chars must succeed"
        );
    }

    #[test]
    fn hex_to_bytes32_valid_uppercase() {
        let s = "A".repeat(64);
        assert!(
            hex_to_bytes32(&s).is_some(),
            "64 valid uppercase hex chars must succeed"
        );
    }

    #[test]
    fn hex_to_bytes32_wrong_length_returns_none() {
        assert!(hex_to_bytes32("").is_none(), "empty string must return None");
        assert!(hex_to_bytes32("deadbeef").is_none(), "8-char hex must return None");
        assert!(
            hex_to_bytes32(&"a".repeat(63)).is_none(),
            "63-char hex must return None"
        );
        assert!(
            hex_to_bytes32(&"a".repeat(65)).is_none(),
            "65-char hex must return None"
        );
    }

    #[test]
    fn hex_to_bytes32_non_hex_char_returns_none() {
        // 63 valid hex chars + one invalid char ('z').
        let mut bad = "a".repeat(63);
        bad.push('z');
        assert_eq!(bad.len(), 64);
        assert!(
            hex_to_bytes32(&bad).is_none(),
            "non-hex character must return None, not silently zero"
        );
    }

    #[test]
    fn hex_to_bytes32_non_hex_middle_returns_none() {
        // Valid-length string with a space in the middle.
        let mut bad = "a".repeat(32);
        bad.push(' ');
        bad.push_str(&"a".repeat(31));
        assert_eq!(bad.len(), 64);
        assert!(
            hex_to_bytes32(&bad).is_none(),
            "space in hex string must return None"
        );
    }

    #[test]
    fn hex_to_bytes32_decodes_known_value() {
        // First byte should be 0xde, second 0xad.
        let s = format!("{}{}", "dead", "a".repeat(60));
        let result = hex_to_bytes32(&s).expect("valid hex");
        assert_eq!(result[0], 0xde);
        assert_eq!(result[1], 0xad);
    }
}
