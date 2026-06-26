//! Primary-kind validation and acquisition-kind compilation (NIP-18 / #1740).
//!
//! Apps declare the primary content kinds they render; repost wrapper kinds
//! (6/16) and the NIP-09 delete kind (5) are protocol mechanics derived here,
//! never declared as primary. This is the single canonical, boundary-safe
//! transform for the FFI / WASM / compiler boundaries.

use std::collections::BTreeSet;

use crate::{is_repost_kind, KIND_DELETE, KIND_GENERIC_REPOST, KIND_REPOST};

/// Error returned when an app-declared primary feed kind is not actually primary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimaryKindError {
    /// Repost wrapper kinds are acquisition mechanics derived from primary
    /// content kinds. Apps must not declare them as primary content.
    RepostWrapper { kind: u32 },
    /// The NIP-09 deletion kind (5) is compiler-derived suppression acquisition,
    /// never a primary content kind an app declares.
    DeleteKind,
    /// No primary kinds were declared. Surfaced by [`validate_primary_kinds`]
    /// (the open-a-feed validator); the permissive
    /// [`try_acquisition_kinds_for_primary`] treats an empty set as the
    /// canonical clear-feed signal instead.
    EmptyPrimaryKinds,
}

/// Compile app-declared primary feed kinds into acquisition kinds.
///
/// Apps declare the content kinds they want to render. Repost wrapper kinds are
/// protocol mechanics: kind `6` for primary kind `1`, and kind `16` for every
/// non-kind-1 primary target.
#[must_use]
pub fn acquisition_kinds_for_primary<I>(primary_kinds: I) -> BTreeSet<u32>
where
    I: IntoIterator<Item = u32>,
{
    try_acquisition_kinds_for_primary(primary_kinds)
        .expect("primary feed kinds must not include repost-wrapper or delete kinds")
}

/// Try to compile app-declared primary feed kinds into acquisition kinds.
///
/// This is the single canonical, boundary-safe transform for FFI/WASM/user
/// input (issue #1740 step 5). It:
///
/// * rejects kind `6` and kind `16` (NIP-18 repost wrappers) as primary kinds —
///   apps say "I render `[1]`" and the wrappers are derived;
/// * rejects kind `5` (NIP-09 deletion) as a primary kind for the same reason;
/// * derives the repost wrapper acquisition kinds (`6` for primary `1`, `16` for
///   every non-`1` primary);
/// * derives kind `5` acquisition for any non-empty feed so live subscriptions
///   receive the deletes that suppress superseded/retracted rows. An empty
///   primary set stays empty — that is the canonical clear-feed signal.
pub fn try_acquisition_kinds_for_primary<I>(
    primary_kinds: I,
) -> Result<BTreeSet<u32>, PrimaryKindError>
where
    I: IntoIterator<Item = u32>,
{
    let mut kinds = BTreeSet::new();
    let mut needs_kind6 = false;
    let mut needs_kind16 = false;

    for kind in primary_kinds {
        if is_repost_kind(kind) {
            return Err(PrimaryKindError::RepostWrapper { kind });
        }
        if kind == KIND_DELETE {
            return Err(PrimaryKindError::DeleteKind);
        }
        kinds.insert(kind);
        match kind {
            1 => needs_kind6 = true,
            _ => needs_kind16 = true,
        }
    }

    if needs_kind6 {
        kinds.insert(KIND_REPOST);
    }
    if needs_kind16 {
        kinds.insert(KIND_GENERIC_REPOST);
    }
    // Deletions suppress superseded/retracted rows, so a live feed must acquire
    // them for the observer's kind:5 handling to fire. But an EMPTY primary set
    // is the canonical "clear this feed" signal — an empty acquisition set
    // withdraws the subscription.
    // Injecting kind:5 there would turn a clear into a deletes-only
    // subscription, so only add it when the feed has primary content to suppress.
    if !kinds.is_empty() {
        kinds.insert(KIND_DELETE);
    }

    Ok(kinds)
}

/// Validate app-declared primary feed kinds for opening a feed, and compile them
/// into the acquisition kind set.
///
/// The single canonical primary-kind validator for the FFI / WASM / compiler
/// boundaries (issue #1740). It is the strict twin of
/// [`try_acquisition_kinds_for_primary`]: identical wrapper/delete rejection and
/// acquisition derivation, but it ALSO rejects an empty primary set
/// ([`PrimaryKindError::EmptyPrimaryKinds`]) — an open feed must declare at least
/// one primary content kind, whereas the permissive transform treats an empty
/// set as the clear-feed signal.
pub fn validate_primary_kinds<I>(primary_kinds: I) -> Result<BTreeSet<u32>, PrimaryKindError>
where
    I: IntoIterator<Item = u32>,
{
    let kinds: Vec<u32> = primary_kinds.into_iter().collect();
    if kinds.is_empty() {
        return Err(PrimaryKindError::EmptyPrimaryKinds);
    }
    try_acquisition_kinds_for_primary(kinds)
}
