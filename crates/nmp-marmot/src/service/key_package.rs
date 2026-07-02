//! The signed kind:30443 KeyPackage publication result returned by
//! [`MarmotService::publish_key_package`](super::MarmotService::publish_key_package).

use nostr::Event;

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
