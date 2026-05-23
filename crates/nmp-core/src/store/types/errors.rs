//! `StoreError` — the single error type for all `EventStore` operations.
//!
//! D6: store errors cross FFI only via `Result<_, StoreError>` translated at
//! the actor boundary to a tagged-union toast payload. They are never surfaced
//! as panics or C exceptions.

use super::gc::ClaimerId;

// ─── VerifyError ─────────────────────────────────────────────────────────────

/// Error returned by `VerifiedEvent::try_from_raw()` when an event fails
/// cryptographic verification (id hash check or Schnorr signature check).
#[derive(Debug)]
pub enum VerifyError {
    /// Event JSON could not be re-serialized for the verifier.
    Serialization(String),
    /// The event id does not match `SHA256(canonical_json)`.
    InvalidId,
    /// The Schnorr signature does not validate against the event id and pubkey.
    InvalidSignature,
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Serialization(s) => write!(f, "verification serialization error: {s}"),
            Self::InvalidId => write!(f, "event id mismatch"),
            Self::InvalidSignature => write!(f, "invalid Schnorr signature"),
        }
    }
}

impl std::error::Error for VerifyError {}

// ─── StoreError ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum StoreError {
    Io(String),
    Corrupt(String),
    Encoding(String),
    SchemaTooNew {
        namespace: String,
        on_disk: u32,
        expected: u32,
    },
    MigrationFailed {
        namespace: String,
        from: u32,
        to: u32,
        reason: String,
    },
    UnknownNamespace(String),
    /// Returned by `claim()` when the per-view or global pinned ceiling is exceeded.
    /// D8 / GC ceiling invariant — see `docs/design/lmdb/gc.md` §2.
    OverPinned {
        claimer: ClaimerId,
        requested: usize,
        ceiling: usize,
    },
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(s) => write!(f, "backend i/o: {s}"),
            Self::Corrupt(s) => write!(f, "backend corruption: {s}"),
            Self::Encoding(s) => write!(f, "encoding: {s}"),
            Self::SchemaTooNew { namespace, on_disk, expected } =>
                write!(f, "schema too new: {namespace} on-disk={on_disk} expected={expected}"),
            Self::MigrationFailed { namespace, from, to, reason } =>
                write!(f, "schema migration failed: {namespace} v{from}->{to}: {reason}"),
            Self::UnknownNamespace(s) => write!(f, "unknown namespace: {s}"),
            Self::OverPinned { claimer, requested, ceiling } =>
                write!(f, "claim ceiling exceeded: claimer={claimer:?} requested={requested} ceiling={ceiling}"),
        }
    }
}

impl std::error::Error for StoreError {}
