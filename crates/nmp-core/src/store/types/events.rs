//! `RawEvent`, `VerifiedEvent`, and `StoredEvent` types.
//!
//! `VerifiedEvent` is the gate type for `EventStore::insert`: only events that
//! have passed Schnorr signature verification can enter the store.

use std::sync::Arc;
use serde::{Deserialize, Serialize};
use super::ids::{EventId, PubKey, hex_to_bytes32, hex_nibble};
use super::errors::VerifyError;

// ─── RawEvent ────────────────────────────────────────────────────────────────

/// Temporary stand-in for `nostr::Event` until the nostr crate is in the workspace.
///
/// Fields match the NIP-01 event object exactly. Signature verification is
/// skipped for now (insert always trusts the caller). The M3-lmdb task will
/// swap this for the real type and enable proper sig checks.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawEvent {
    pub id: String,          // lowercase hex
    pub pubkey: String,      // lowercase hex
    pub created_at: u64,     // unix seconds
    pub kind: u32,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub sig: String,         // lowercase hex
}

impl RawEvent {
    /// Decode hex id → 32 bytes. Returns zeroes on malformed input.
    pub fn id_bytes(&self) -> EventId {
        hex_to_bytes32(&self.id)
    }

    /// Decode hex pubkey → 32 bytes. Returns zeroes on malformed input.
    pub fn pubkey_bytes(&self) -> PubKey {
        hex_to_bytes32(&self.pubkey)
    }

    /// NIP-01 replaceable kinds: 0, 3, and 10000–19999.
    pub fn is_replaceable(&self) -> bool {
        self.kind == 0 || self.kind == 3 || (10_000..20_000).contains(&self.kind)
    }

    /// NIP-33 parameterized replaceable kinds: 30000–39999.
    pub fn is_param_replaceable(&self) -> bool {
        (30_000..40_000).contains(&self.kind)
    }

    /// NIP-16 ephemeral kinds: 20000–29999.
    pub fn is_ephemeral(&self) -> bool {
        (20_000..30_000).contains(&self.kind)
    }

    /// Returns the value of the first `d` tag, if present.
    pub fn d_tag(&self) -> Option<Vec<u8>> {
        self.tags
            .iter()
            .find(|t| t.first().map(|s| s == "d").unwrap_or(false))
            .and_then(|t| t.get(1))
            .map(|s| s.as_bytes().to_vec())
    }

    /// Returns the unix-second value of the first `expiration` tag, if present.
    pub fn expiration(&self) -> Option<u64> {
        self.tags
            .iter()
            .find(|t| t.first().map(|s| s == "expiration").unwrap_or(false))
            .and_then(|t| t.get(1))
            .and_then(|s| s.parse::<u64>().ok())
    }

    /// Returns all `e`-tag target ids (lowercase hex).
    pub fn e_tags(&self) -> Vec<String> {
        self.tags
            .iter()
            .filter(|t| t.first().map(|s| s == "e").unwrap_or(false))
            .filter_map(|t| t.get(1).cloned())
            .collect()
    }

    /// Returns all `p`-tag target pubkeys (lowercase hex).
    pub fn p_tags(&self) -> Vec<String> {
        self.tags
            .iter()
            .filter(|t| t.first().map(|s| s == "p").unwrap_or(false))
            .filter_map(|t| t.get(1).cloned())
            .collect()
    }

    /// Returns all `a`-tag target addresses (e.g. "30023:pubkey:dtag").
    pub fn a_tags(&self) -> Vec<String> {
        self.tags
            .iter()
            .filter(|t| t.first().map(|s| s == "a").unwrap_or(false))
            .filter_map(|t| t.get(1).cloned())
            .collect()
    }

    /// Validates the event has a plausible structure (non-empty id, pubkey, sig).
    /// Full cryptographic verification is deferred until the nostr crate is wired in.
    pub fn is_structurally_valid(&self) -> bool {
        self.id.len() == 64 && self.pubkey.len() == 64 && self.sig.len() == 128
    }

    /// Hex-decode this event's id. Used internally by mem/insert.rs.
    pub(crate) fn hex_to_bytes32_owned(s: &str) -> [u8; 32] {
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
}

// ─── VerifiedEvent ───────────────────────────────────────────────────────────

/// A `RawEvent` that has passed cryptographic verification (id hash + Schnorr
/// signature). This is the only type accepted by `EventStore::insert`.
///
/// Construction is intentionally limited to `try_from_raw()`. In tests and
/// integration-test harnesses, `from_raw_unchecked()` bypasses verification
/// (gated on `cfg(any(test, feature = "test-support"))`).
pub struct VerifiedEvent(pub(crate) RawEvent);

impl VerifiedEvent {
    /// Verify `raw` and, if valid, wrap it in `VerifiedEvent`.
    ///
    /// Internally serializes `raw` to the NIP-01 canonical JSON, parses it
    /// with the `nostr` crate, and calls `nostr::Event::verify()` which checks
    /// both the event-id hash and the Schnorr signature.
    pub fn try_from_raw(raw: RawEvent) -> Result<Self, VerifyError> {
        use nostr::util::JsonUtil as _;
        let json = serde_json::to_string(&raw)
            .map_err(|e| VerifyError::Serialization(e.to_string()))?;
        let ev = nostr::Event::from_json(&json)
            .map_err(|_| VerifyError::InvalidId)?;
        // verify() checks both event-id hash and Schnorr signature.
        ev.verify().map_err(|e| {
            let msg = e.to_string();
            if msg.contains("id") {
                VerifyError::InvalidId
            } else {
                VerifyError::InvalidSignature
            }
        })?;
        Ok(VerifiedEvent(raw))
    }

    /// Bypass verification — only available in test and integration-test builds.
    ///
    /// Use this in store harnesses and unit tests where synthetic events with
    /// placeholder signatures are needed. NEVER enabled in production builds.
    #[cfg(any(test, feature = "test-support"))]
    pub fn from_raw_unchecked(raw: RawEvent) -> Self {
        VerifiedEvent(raw)
    }

    /// Access the underlying raw event.
    pub fn raw(&self) -> &RawEvent {
        &self.0
    }

    /// Consume and return the underlying raw event.
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
    pub received_at_ms: u64,   // wall-clock first arrival across all relays
}
