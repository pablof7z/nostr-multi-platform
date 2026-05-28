//! [`ClaimRequest`] — the engine's outbound hydration signal.
//!
//! When the engine sees a qualifying reference to a root it does not hold
//! locally, it emits `Claim` so the *wiring layer* can fetch the root through
//! whatever hydration primitive the host exposes (in the NIP-10 instance:
//! `nmp_app_claim_event`). When the root is no longer wanted, it emits
//! `Release`. The engine deliberately does NOT depend on the action system or
//! any C-ABI surface — it asks through a closure sink (D7: "the engine asks;
//! the wiring decides").
//!
//! A claim carries a [`ThreadPointer`] (not a bare id) so the wiring layer can
//! encode the correct NIP-19 URI shape — `Event` → `nevent`, `Address` →
//! `naddr`, `External` → terminal (never emitted as a `Claim`).

use nmp_core::planner::RelayHint;
use nmp_threading::pointer::ThreadPointer;

/// A hydration request the engine emits through its construction-time sink.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClaimRequest {
    /// Ask the host to fetch the event/address named by `pointer`, seeding the
    /// first REQ with `hints`. `consumer_id` is the refcount key the host uses
    /// to de-dupe overlapping claims and to match a later `Release`.
    Claim {
        pointer: ThreadPointer,
        hints: Vec<RelayHint>,
        consumer_id: String,
    },
    /// Release a prior claim. Emitted when the root becomes locally available
    /// (no longer needs fetching) or when the engine evicts the last pending
    /// reference under D5 capacity pressure.
    Release {
        pointer: ThreadPointer,
        consumer_id: String,
    },
}

impl ClaimRequest {
    /// The pointer this request concerns.
    #[must_use]
    pub fn pointer(&self) -> &ThreadPointer {
        match self {
            Self::Claim { pointer, .. } | Self::Release { pointer, .. } => pointer,
        }
    }
}
