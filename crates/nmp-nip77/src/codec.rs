//! Byte/string helpers around the upstream `nostr` NIP-77 frame types.

/// Hex payload parse error.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum HexError {
    /// Hex strings must contain full bytes.
    OddLength,
    /// One character was outside `[0-9a-fA-F]`.
    InvalidNibble,
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    static HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

pub(crate) fn hex_decode(s: &str) -> Result<Vec<u8>, HexError> {
    if !s.len().is_multiple_of(2) {
        return Err(HexError::OddLength);
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for pair in s.as_bytes().chunks(2) {
        out.push((hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?);
    }
    Ok(out)
}

fn hex_nibble(b: u8) -> Result<u8, HexError> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(HexError::InvalidNibble),
    }
}

pub(crate) fn notice_mentions_negentropy(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("negentropy")
        || lower.contains("neg-open")
        || lower.contains("neg_msg")
        || lower.contains("bad msg")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip_accepts_uppercase() {
        assert_eq!(hex_encode(&[0x60, 0xaa, 0xbb]), "60aabb");
        assert_eq!(hex_decode("60AABB").unwrap(), vec![0x60, 0xaa, 0xbb]);
    }

    #[test]
    fn notice_detection_matches_real_unsupported_shape() {
        assert!(notice_mentions_negentropy(
            "ERROR: bad msg: negentropy disabled"
        ));
    }
}
