//! `NaddrCoord` — parameterized-replaceable event coordinate (PRE address triple).
//!
//! Owns the addressable-event coordinate used by `InterestShape::addresses`
//! for address-pointer hydration and by the D8 composite reverse index to
//! deduplicate address-pointer interests across views.

use serde::{Deserialize, Serialize};

use super::Pubkey;

// ─── NaddrCoord ──────────────────────────────────────────────────────────────

/// A parameterized-replaceable event coordinate: the triple that uniquely
/// identifies an addressable event (kinds 10000–19999, 30000–39999) across
/// all relays. Equivalent to the `naddr` bech32 encoding without the relay hint.
///
/// Used by `InterestShape::addresses` for address-pointer hydration (Rule 8
/// of the merge lattice) and by the D8 composite reverse index to deduplicate
/// address-pointer interests across views.
///
/// Design: `docs/design/subscription-compilation/intro.md` §2.1
#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct NaddrCoord {
    /// Author of the addressed event.
    pub pubkey: Pubkey,
    /// Addressable kind (10000–19999 or 30000–39999).
    pub kind: u32,
    /// The `d` tag value; empty string for events with no `d` tag.
    pub d_tag: String,
}

// Phase 2 (nmp-nostr-id): NaddrCoord::from_naddr_bech32 / to_naddr_bech32 helpers
// land when the nmp-nostr-id bech32 codec crate joins the workspace. Both helpers
// are needed for `nmp_nip01::ThreadView` and `nmp_nip01::Nip10ModularTimelineView`
// (the latter wrapping `nmp_threading::Grouper`) to accept user-facing naddr
// strings from the host-language FFI surface.
