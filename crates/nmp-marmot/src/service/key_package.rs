//! The signed kind:30443 KeyPackage publication result returned by
//! [`MarmotService::publish_key_package`](super::MarmotService::publish_key_package).

use nostr::{Event, PublicKey};

/// Stable NIP-33 `d`-tag slot id for an identity's advertised MLS last-resort
/// key package (kind:30443).
///
/// Deterministically derived from the identity pubkey via a domain-separated
/// SHA-256 so it is STABLE across store re-creations (a random `d`, MDK's
/// default, is not — see `MarmotService::publish_key_package` for the #3057
/// rationale: republishes must REPLACE, not accumulate, so relays never serve a
/// key package whose private half is missing from the invitee's current store).
///
/// MDK requires the `d` tag to be exactly 64 hex chars (32 bytes); a SHA-256
/// digest satisfies that exactly. The NIP-33 address is `(kind, pubkey, d)`, so
/// one deterministic `d` per identity yields exactly one long-lived,
/// self-replacing key-package slot.
#[must_use]
pub(crate) fn marmot_key_package_d_tag(pubkey: &PublicKey) -> String {
    use nostr::hashes::{sha256, Hash};
    let mut preimage = b"nmp-marmot:key-package-slot:v1:".to_vec();
    preimage.extend_from_slice(pubkey.to_hex().as_bytes());
    sha256::Hash::hash(&preimage).to_string()
}

/// The signed Nostr event to publish for one KeyPackage publication.
/// Published exclusively as kind:30443 (NIP-33 addressable). `d_tag`
/// and `hash_ref` are surfaced for the rotation lifecycle (plan §Step 3).
///
/// The legacy kind:443 dual-publish was retired 2026-05-31 per the deadline
/// in the original MDK spec. Only kind:30443 is published and subscribed.
#[derive(Debug)]
pub struct KeyPackagePublication {
    /// Signed kind:30443 event (current spec, NIP-33 addressable).
    pub event_30443: Event,
    /// The `d` tag value — store and reuse on rotation for relay replacement.
    pub d_tag: String,
    /// postcard-serialized `KeyPackageRef` bytes for consumption tracking.
    pub hash_ref: Vec<u8>,
}
