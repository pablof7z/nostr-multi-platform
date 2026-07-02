//! `MarmotService`'s error type and `Result` alias.
//!
//! Wraps `mdk_core::Error` (kept opaque as a string so the error type does
//! not leak MLS types across the protocol boundary) plus service-level
//! validation and the `PendingGroupChange`/`CreateGroupPending` orphaned-
//! commit diagnostic (V-61).

/// Errors surfaced by the service. Wraps `mdk_core::Error` (kept opaque as a
/// string so the error type does not leak MLS types across the protocol
/// boundary) plus service-level validation.
#[derive(Debug)]
pub enum MarmotError {
    /// An underlying MDK / MLS error (stringified to keep MLS types in-crate).
    Mdk(String),
    /// A Nostr event construction / signing error.
    Nostr(String),
    /// A NIP-59 gift-wrap / unwrap error.
    GiftWrap(String),
    /// Service-level invariant violation.
    Invariant(String),
    /// A `PendingGroupChange` was dropped without being committed or cleared.
    ///
    /// The pending commit was defensively cleared in `Drop`, but the
    /// kind:445/commit event was never published to the relay — local MLS
    /// state and the relay-published epoch have diverged. The host must block
    /// further group sends until the operator resolves the divergence (e.g.
    /// via a `self_update` re-sync or by rejoining the group).
    OrphanedCommit {
        /// Hex-encoded MLS group id the orphaned commit belongs to.
        group_id_hex: String,
    },
}

impl std::fmt::Display for MarmotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mdk(s) => write!(f, "mdk error: {s}"),
            Self::Nostr(s) => write!(f, "nostr error: {s}"),
            Self::GiftWrap(s) => write!(f, "nip59 error: {s}"),
            Self::Invariant(s) => write!(f, "invariant violation: {s}"),
            Self::OrphanedCommit { group_id_hex } => write!(
                f,
                "orphaned MLS commit for group {group_id_hex}: \
                 PendingGroupChange dropped without commit/clear; \
                 local state may have diverged from the relay-published epoch"
            ),
        }
    }
}
impl std::error::Error for MarmotError {}

impl From<mdk_core::Error> for MarmotError {
    fn from(e: mdk_core::Error) -> Self {
        Self::Mdk(e.to_string())
    }
}
impl From<nmp_nip59::Nip59Error> for MarmotError {
    fn from(e: nmp_nip59::Nip59Error) -> Self {
        Self::GiftWrap(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, MarmotError>;
