//! F-TTL replaceable event freshness tracking for LMDB.
//!
//! Per the F-TTL design, we track `check_again_after` (unix milliseconds) for each
//! replaceable event identity. This allows the kernel to know when a cached replaceable
//! event may have been updated on relays.
//!
//! ## Key design
//!
//! Regular replaceable: `(kind: u32, pubkey: [u8; 32])` → key=[4B BE kind][32B pubkey]
//! Parameterized replaceable: `(kind: u32, pubkey: [u8; 32], d_tag: String)` → key=[4B BE kind][32B pubkey][utf8 d_tag]
//!
//! Value: `[8B BE check_again_after_unix_ms]`
//!
//! In-memory cache: `HashMap<ReplaceableKey, u64>` hot-loaded on open().

use std::collections::HashMap;

/// Uniquely identifies a replaceable event.
///
/// NIP-01 distinguishes:
/// - Regular replaceable: kinds 0–9999, 10000–19999 (key = kind + pubkey)
/// - Parameterized replaceable: kinds 20000–29999, 30000–39999 (key = kind + pubkey + d_tag)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReplaceableKey {
    /// Regular replaceable event: identified by kind and author pubkey.
    Regular { kind: u32, pubkey: [u8; 32] },
    /// Parameterized replaceable event: identified by kind, author pubkey, and d-tag.
    Parameterized {
        kind: u32,
        pubkey: [u8; 32],
        d_tag: String,
    },
}

impl ReplaceableKey {
    /// Serialize to LMDB key bytes: kind[4B BE] || pubkey[32B] || d_tag_utf8[variable]
    pub fn to_lmdb_key(&self) -> Vec<u8> {
        let mut key = Vec::with_capacity(36 + 64); // 4 + 32 + max utf8 estimate

        // Kind (4 bytes, big-endian)
        let kind = match self {
            Self::Regular { kind, .. } | Self::Parameterized { kind, .. } => *kind,
        };
        key.extend_from_slice(&kind.to_be_bytes());

        // Pubkey (32 bytes)
        let pubkey = match self {
            Self::Regular { pubkey, .. } | Self::Parameterized { pubkey, .. } => pubkey,
        };
        key.extend_from_slice(pubkey);

        // D-tag (variable length UTF-8, only for parameterized)
        if let Self::Parameterized { d_tag, .. } = self {
            key.extend_from_slice(d_tag.as_bytes());
        }

        key
    }

    /// Deserialize from LMDB key bytes (inverse of `to_lmdb_key`).
    /// Requires the `kind` to distinguish regular vs parameterized.
    pub fn from_lmdb_key(key_bytes: &[u8], kind: u32) -> Result<Self, String> {
        if key_bytes.len() < 36 {
            return Err("key too short (need at least 36 bytes for kind+pubkey)".to_string());
        }

        let mut pubkey = [0u8; 32];
        pubkey.copy_from_slice(&key_bytes[4..36]);

        if is_parameterized_replaceable(kind) {
            if key_bytes.len() <= 36 {
                return Err("parameterized replaceable must have d_tag".to_string());
            }
            let d_tag = String::from_utf8(key_bytes[36..].to_vec())
                .map_err(|e| format!("d_tag not valid utf8: {e}"))?;
            Ok(Self::Parameterized { kind, pubkey, d_tag })
        } else {
            Ok(Self::Regular { kind, pubkey })
        }
    }
}

/// Return whether a kind is parameterized replaceable (NIP-01).
pub fn is_parameterized_replaceable(kind: u32) -> bool {
    (kind >= 20000 && kind < 30000) || (kind >= 30000 && kind < 40000)
}

/// Return whether a kind is replaceable (NIP-01).
/// This includes both regular (0-19999) and parameterized (20000-39999) replaceable kinds.
pub fn is_replaceable(kind: u32) -> bool {
    (kind < 20000) || (kind >= 30000 && kind < 40000)
}

/// In-memory cache for replaceable freshness timestamps.
/// Maps `ReplaceableKey` → `check_again_after_unix_ms`.
pub type ReplaceableCache = HashMap<ReplaceableKey, u64>;

/// Encode check_again_after timestamp to 8 bytes (big-endian u64).
pub fn encode_timestamp(ts_ms: u64) -> Vec<u8> {
    ts_ms.to_be_bytes().to_vec()
}

/// Decode check_again_after timestamp from bytes.
pub fn decode_timestamp(bytes: &[u8]) -> Result<u64, String> {
    if bytes.len() < 8 {
        return Err(format!("timestamp bytes too short: {} < 8", bytes.len()));
    }
    Ok(u64::from_be_bytes(bytes[..8].try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_regular_key_serialization() {
        let key = ReplaceableKey::Regular {
            kind: 0,
            pubkey: [1u8; 32],
        };
        let bytes = key.to_lmdb_key();
        assert_eq!(bytes.len(), 36); // 4 + 32
        assert_eq!(&bytes[0..4], &0u32.to_be_bytes());
        assert_eq!(&bytes[4..36], &[1u8; 32]);
    }

    #[test]
    fn test_parameterized_key_serialization() {
        let key = ReplaceableKey::Parameterized {
            kind: 30023,
            pubkey: [2u8; 32],
            d_tag: "my-article".to_string(),
        };
        let bytes = key.to_lmdb_key();
        assert_eq!(bytes.len(), 36 + 10); // 4 + 32 + "my-article"
        assert_eq!(&bytes[0..4], &30023u32.to_be_bytes());
        assert_eq!(&bytes[4..36], &[2u8; 32]);
        assert_eq!(&bytes[36..], b"my-article");
    }

    #[test]
    fn test_timestamp_encoding() {
        let ts = 1234567890u64;
        let encoded = encode_timestamp(ts);
        assert_eq!(encoded.len(), 8);
        let decoded = decode_timestamp(&encoded).unwrap();
        assert_eq!(decoded, ts);
    }

    #[test]
    fn test_is_replaceable() {
        // Regular replaceable
        assert!(is_replaceable(0));
        assert!(is_replaceable(9999));
        assert!(is_replaceable(10000));
        assert!(is_replaceable(19999));

        // Not replaceable
        assert!(!is_replaceable(20000));
        assert!(!is_replaceable(29999));

        // Parameterized replaceable
        assert!(is_replaceable(30023));
        assert!(is_replaceable(30000));
        assert!(is_replaceable(39999));

        // Not replaceable
        assert!(!is_replaceable(40000));
    }

    #[test]
    fn test_is_parameterized_replaceable() {
        assert!(is_parameterized_replaceable(20000));
        assert!(is_parameterized_replaceable(29999));
        assert!(is_parameterized_replaceable(30000));
        assert!(is_parameterized_replaceable(39999));

        assert!(!is_parameterized_replaceable(0));
        assert!(!is_parameterized_replaceable(10000));
        assert!(!is_parameterized_replaceable(40000));
    }
}
