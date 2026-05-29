//! `RawEvent`, `VerifiedEvent`, and `StoredEvent` types.
//!
//! `VerifiedEvent` is the gate type for `EventStore::insert`: only events that
//! have passed Schnorr signature verification can enter the store.

use super::errors::VerifyError;
use super::ids::{hex_to_bytes32, EventId, PubKey};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ─── RawEvent ────────────────────────────────────────────────────────────────

/// NIP-01 event wire type used as the store's internal representation.
///
/// Fields match the NIP-01 event object exactly. Signature verification is
/// enforced by [`VerifiedEvent::try_from_raw`], which uses `nostr::Event::verify()`
/// (Schnorr signature + event-id hash) before any event enters the store.
/// The store only accepts `VerifiedEvent`; `RawEvent` is never inserted
/// directly.
///
/// M3-lmdb: when the LMDB store lands, this may be replaced by `nostr::Event`
/// natively, eliminating the JSON round-trip in `try_from_raw`. Until then
/// this type is NOT a security gap — verification happens at the boundary.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawEvent {
    pub id: String,      // lowercase hex
    pub pubkey: String,  // lowercase hex
    pub created_at: u64, // unix seconds
    pub kind: u32,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub sig: String, // lowercase hex
}

impl RawEvent {
    /// Decode hex id → 32 bytes.
    ///
    /// Returns `None` if `self.id` is not a valid 64-character hex string.
    /// Call sites on verified events (post-Schnorr) may `.expect()` with a
    /// clear message; call sites on unverified input must propagate or skip.
    #[must_use]
    pub fn id_bytes(&self) -> Option<EventId> {
        hex_to_bytes32(&self.id)
    }

    /// Decode hex pubkey → 32 bytes.
    ///
    /// Returns `None` if `self.pubkey` is not a valid 64-character hex string.
    /// Call sites on verified events (post-Schnorr) may `.expect()` with a
    /// clear message; call sites on unverified input must propagate or skip.
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

    /// Returns the value of the first `d` tag, if present.
    #[must_use]
    pub fn d_tag(&self) -> Option<Vec<u8>> {
        self.tags
            .iter()
            .find(|t| t.first().is_some_and(|s| s == "d"))
            .and_then(|t| t.get(1))
            .map(|s| s.as_bytes().to_vec())
    }

    /// Returns the unix-second value of the first `expiration` tag, if present.
    #[must_use]
    pub fn expiration(&self) -> Option<u64> {
        self.tags
            .iter()
            .find(|t| t.first().is_some_and(|s| s == "expiration"))
            .and_then(|t| t.get(1))
            .and_then(|s| s.parse::<u64>().ok())
    }

    /// Returns all `e`-tag target ids (lowercase hex).
    #[must_use]
    pub fn e_tags(&self) -> Vec<String> {
        self.tags
            .iter()
            .filter(|t| t.first().is_some_and(|s| s == "e"))
            .filter_map(|t| t.get(1).cloned())
            .collect()
    }

    /// Returns all `p`-tag target pubkeys (lowercase hex).
    #[must_use]
    pub fn p_tags(&self) -> Vec<String> {
        self.tags
            .iter()
            .filter(|t| t.first().is_some_and(|s| s == "p"))
            .filter_map(|t| t.get(1).cloned())
            .collect()
    }

    /// Returns all `a`-tag target addresses (e.g. "30023:pubkey:dtag").
    #[must_use]
    pub fn a_tags(&self) -> Vec<String> {
        self.tags
            .iter()
            .filter(|t| t.first().is_some_and(|s| s == "a"))
            .filter_map(|t| t.get(1).cloned())
            .collect()
    }

    /// Returns true iff the event has valid field lengths and hex-decodable
    /// `id` and `pubkey` strings. This is a cheap pre-filter only —
    /// cryptographic verification is done by `VerifiedEvent::try_from_raw`.
    ///
    /// Checking hex here ensures that `id_bytes()` / `pubkey_bytes()` are
    /// guaranteed to return `Some` for any event that passes this gate.
    #[must_use]
    pub fn is_structurally_valid(&self) -> bool {
        self.sig.len() == 128
            && hex_to_bytes32(&self.id).is_some()
            && hex_to_bytes32(&self.pubkey).is_some()
    }

    /// Hex-decode an arbitrary 64-hex string to 32 bytes.
    ///
    /// Returns `None` on bad length or non-hex characters.
    /// Used internally by `mem/insert.rs` and `lmdb/insert.rs` for tag-derived
    /// hex strings that are not Schnorr-verified and may be malformed.
    pub(crate) fn hex_to_bytes32_owned(s: &str) -> Option<[u8; 32]> {
        hex_to_bytes32(s)
    }
}

// ─── VerifiedEvent ───────────────────────────────────────────────────────────

/// A `RawEvent` that has passed cryptographic verification (id hash + Schnorr
/// signature). This is the only type accepted by `EventStore::insert`.
///
/// Construction is intentionally limited to `try_from_raw()`. In tests and
/// integration-test harnesses, `from_raw_unchecked()` bypasses verification
/// (gated on `cfg(any(test, feature = "test-support"))`).
#[derive(Clone, Debug)]
pub struct VerifiedEvent(pub(crate) RawEvent);

impl VerifiedEvent {
    /// Verify `raw` and, if valid, wrap it in `VerifiedEvent`.
    ///
    /// Internally serializes `raw` to the NIP-01 canonical JSON, parses it
    /// with the `nostr` crate, and calls `nostr::Event::verify()` which checks
    /// both the event-id hash and the Schnorr signature.
    pub fn try_from_raw(raw: RawEvent) -> Result<Self, VerifyError> {
        use nostr::util::JsonUtil as _;
        let json =
            serde_json::to_string(&raw).map_err(|e| VerifyError::Serialization(e.to_string()))?;
        let ev = nostr::Event::from_json(&json).map_err(|_| VerifyError::InvalidId)?;
        // verify() checks both event-id hash and Schnorr signature.
        ev.verify().map_err(|e| {
            let msg = e.to_string();
            if msg.contains("id") {
                VerifyError::InvalidId
            } else {
                VerifyError::InvalidSignature
            }
        })?;
        Ok(Self(raw))
    }

    /// Bypass verification — only available in test and integration-test builds.
    ///
    /// Use this in store harnesses and unit tests where synthetic events with
    /// placeholder signatures are needed. NEVER enabled in production builds.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn from_raw_unchecked(raw: RawEvent) -> Self {
        VerifiedEvent(raw)
    }

    /// Access the underlying raw event.
    #[must_use]
    pub fn raw(&self) -> &RawEvent {
        &self.0
    }

    /// Consume and return the underlying raw event.
    #[must_use]
    pub fn into_raw(self) -> RawEvent {
        self.0
    }
}

// ─── StoredEvent ─────────────────────────────────────────────────────────────

/// A stored Nostr event with arrival metadata.
///
/// `raw` is `Arc<RawEvent>` so the hot LRU can hold reference-counted copies
/// without cloning the event body on each `get_by_id`.
#[derive(Clone, Debug)]
pub struct StoredEvent {
    pub raw: Arc<RawEvent>,
    pub received_at_ms: u64, // wall-clock first arrival across all relays
}
