//! Wire ⇆ SQLite-row codec for the OPFS-SQLite engine (#1007 PR-3).
//!
//! This module is **target-agnostic and pure** — no shim, no SQLite — so its
//! round-trip is unit-tested on native (see the `tests` module). The wasm
//! insert / read paths in [`crate::insert`] / [`crate::store_impl`] call this to
//! turn an event into a primary row (and its extracted index keys) and to
//! reconstruct an event from the stored blob.
//!
//! ## Event representation
//!
//! [`EngineEvent`] is a field-for-field mirror of `nmp_store::RawEvent` (the
//! NIP-01 wire object). The crate cannot depend on `nmp-store` (Cargo cycle), so
//! the `nmp-store` wrapper maps `RawEvent` ⇆ `EngineEvent` 1:1 at the seam — the
//! same pattern by which `nmp-store/src/lmdb/conv.rs` maps `RawEvent` ⇆
//! `nostr::Event` for the LMDB engine. Because the serde field names match
//! NIP-01 exactly, the blob this codec writes is byte-identical to the canonical
//! JSON the LMDB backend round-trips, so a database is portable across backends.
//!
//! ## Blob format
//!
//! The `events.raw` column stores the event as canonical NIP-01 JSON
//! (`serde_json`), mirroring the LMDB backend's JSON round-trip (ADR-0012). The
//! arrival metadata (`received_at_ms`) is **not** part of the event; it is a
//! separate primary-row column so [`StoredEngineEvent`] can be reconstructed
//! without polluting the wire blob.

use serde::{Deserialize, Serialize};

use crate::error::SqliteWasmError;
use crate::outcome::{EventId, PubKey};

/// NIP-01 event object — the engine's internal event representation.
///
/// A field-for-field mirror of `nmp_store::RawEvent`; the `nmp-store` wrapper
/// converts between the two at the cycle-free seam.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineEvent {
    /// Lowercase-hex event id (64 chars).
    pub id: String,
    /// Lowercase-hex author pubkey (64 chars).
    pub pubkey: String,
    /// Unix seconds.
    pub created_at: u64,
    /// NIP-01 kind.
    pub kind: u32,
    /// Tag rows (each a non-empty string vector).
    pub tags: Vec<Vec<String>>,
    /// Free-form content.
    pub content: String,
    /// Lowercase-hex Schnorr signature (128 chars).
    pub sig: String,
}

/// A stored event plus its first-arrival wall-clock timestamp (the engine's
/// equivalent of `nmp_store::StoredEvent`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredEngineEvent {
    /// The event itself.
    pub event: EngineEvent,
    /// Wall-clock first arrival across all relays, unix milliseconds.
    pub received_at_ms: u64,
}

impl EngineEvent {
    /// Decode the hex `id` to 32 bytes, or `None` if not 64-char hex.
    #[must_use]
    pub fn id_bytes(&self) -> Option<EventId> {
        hex_to_bytes32(&self.id)
    }

    /// Decode the hex `pubkey` to 32 bytes, or `None` if not 64-char hex.
    #[must_use]
    pub fn pubkey_bytes(&self) -> Option<PubKey> {
        hex_to_bytes32(&self.pubkey)
    }

    /// NIP-01 replaceable kinds: 0, 3, and 10000–19999.
    #[must_use]
    pub fn is_replaceable(&self) -> bool {
        self.kind == 0 || self.kind == 3 || (10_000..20_000).contains(&self.kind)
    }

    /// NIP-33 parameterized replaceable kinds: 30000–39999.
    #[must_use]
    pub fn is_param_replaceable(&self) -> bool {
        (30_000..40_000).contains(&self.kind)
    }

    /// NIP-16 ephemeral kinds: 20000–29999.
    #[must_use]
    pub fn is_ephemeral(&self) -> bool {
        (20_000..30_000).contains(&self.kind)
    }

    /// The first `d`-tag value (raw bytes), if present. Empty/absent → `None`
    /// only when the tag is missing; a present-but-empty `d` yields `Some([])`.
    #[must_use]
    pub fn d_tag(&self) -> Option<Vec<u8>> {
        self.tags
            .iter()
            .find(|t| t.first().is_some_and(|s| s == "d"))
            .map(|t| t.get(1).map(|s| s.as_bytes().to_vec()).unwrap_or_default())
    }

    /// The first `expiration` tag as unix seconds (NIP-40), if parseable.
    #[must_use]
    pub fn expiration(&self) -> Option<u64> {
        self.tags
            .iter()
            .find(|t| t.first().is_some_and(|s| s == "expiration"))
            .and_then(|t| t.get(1))
            .and_then(|s| s.parse::<u64>().ok())
    }

    /// Every single-letter tag `(name, value)` pair — the set the LMDB backend
    /// indexes into `tci` / `atci` / `ktci`. A tag qualifies when its name is
    /// exactly one ASCII-alphabetic char and it carries a value element.
    #[must_use]
    pub fn single_letter_tags(&self) -> Vec<(char, &str)> {
        self.tags
            .iter()
            .filter_map(|t| {
                let name = t.first()?;
                let value = t.get(1)?;
                let mut chars = name.chars();
                let c = chars.next()?;
                if chars.next().is_none() && c.is_ascii_alphabetic() {
                    Some((c, value.as_str()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// All `e`-tag target ids (raw value strings, lowercase hex by convention).
    #[must_use]
    pub fn e_tags(&self) -> Vec<&str> {
        self.tag_values("e")
    }

    /// All `a`-tag coordinate strings (e.g. `"30023:pubkey:dtag"`).
    #[must_use]
    pub fn a_tags(&self) -> Vec<&str> {
        self.tag_values("a")
    }

    fn tag_values(&self, name: &str) -> Vec<&str> {
        self.tags
            .iter()
            .filter(|t| t.first().is_some_and(|s| s == name))
            .filter_map(|t| t.get(1).map(String::as_str))
            .collect()
    }

    /// Cheap structural gate: 128-char sig and hex-decodable id + pubkey. After
    /// this returns `true`, [`Self::id_bytes`] / [`Self::pubkey_bytes`] are
    /// guaranteed `Some`. Does **not** verify the signature — verification is the
    /// caller's responsibility before the event reaches the engine.
    #[must_use]
    pub fn is_structurally_valid(&self) -> bool {
        self.sig.len() == 128 && self.id_bytes().is_some() && self.pubkey_bytes().is_some()
    }
}

/// Serialize an event to its `events.raw` blob (canonical NIP-01 JSON bytes).
pub fn encode_blob(event: &EngineEvent) -> Result<Vec<u8>, SqliteWasmError> {
    serde_json::to_vec(event).map_err(|e| SqliteWasmError::Encoding(format!("encode event: {e}")))
}

/// Reconstruct an event from its `events.raw` blob.
pub fn decode_blob(blob: &[u8]) -> Result<EngineEvent, SqliteWasmError> {
    serde_json::from_slice(blob).map_err(|e| SqliteWasmError::Encoding(format!("decode event: {e}")))
}

/// Decode a 64-char lowercase/uppercase hex string into 32 bytes. Returns `None`
/// on wrong length or non-hex input.
#[must_use]
pub fn hex_to_bytes32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    let bytes = s.as_bytes();
    for (i, slot) in out.iter_mut().enumerate() {
        let hi = hex_val(bytes[i * 2])?;
        let lo = hex_val(bytes[i * 2 + 1])?;
        *slot = (hi << 4) | lo;
    }
    Some(out)
}

#[inline]
fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> EngineEvent {
        EngineEvent {
            id: "a".repeat(64),
            pubkey: "b".repeat(64),
            created_at: 1_700_000_000,
            kind: 30023,
            tags: vec![
                vec!["d".into(), "slug-1".into()],
                vec!["e".into(), "c".repeat(64)],
                vec!["p".into(), "d".repeat(64)],
                vec!["expiration".into(), "1700001000".into()],
                vec!["nonsingle".into(), "x".into()],
            ],
            content: "hello \"world\" — ünïcode".into(),
            sig: "f".repeat(128),
        }
    }

    #[test]
    fn blob_round_trips() {
        let ev = sample();
        let blob = encode_blob(&ev).expect("encode");
        let back = decode_blob(&blob).expect("decode");
        assert_eq!(ev, back);
    }

    #[test]
    fn decode_rejects_garbage() {
        let err = decode_blob(b"not json").unwrap_err();
        assert!(matches!(err, SqliteWasmError::Encoding(_)));
    }

    #[test]
    fn hex_decode_roundtrip() {
        assert_eq!(hex_to_bytes32(&"a".repeat(64)), Some([0xaau8; 32]));
        assert_eq!(hex_to_bytes32(&"A".repeat(64)), Some([0xaau8; 32]));
        assert_eq!(hex_to_bytes32("zz"), None);
        assert_eq!(hex_to_bytes32(&"g".repeat(64)), None);
    }

    #[test]
    fn extracts_index_dimensions() {
        let ev = sample();
        assert!(ev.is_param_replaceable());
        assert_eq!(ev.d_tag(), Some(b"slug-1".to_vec()));
        assert_eq!(ev.expiration(), Some(1_700_001_000));
        // single-letter tags: d, e, p (not "expiration", not "nonsingle").
        let mut names: Vec<char> = ev.single_letter_tags().into_iter().map(|(n, _)| n).collect();
        names.sort_unstable();
        assert_eq!(names, vec!['d', 'e', 'p']);
        assert_eq!(ev.e_tags().len(), 1);
        assert_eq!(ev.a_tags().len(), 0);
    }

    #[test]
    fn structural_gate() {
        assert!(sample().is_structurally_valid());
        let mut bad = sample();
        bad.sig = "f".repeat(127);
        assert!(!bad.is_structurally_valid());
    }
}
