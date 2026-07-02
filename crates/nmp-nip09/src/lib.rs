//! `nmp-nip09` — NIP-09 generic deletion (kind:5) artifact ownership for NMP.
//!
//! This crate is the **exclusive positive owner** of the NIP-09 kind:5
//! deletion wire grammar. It provides:
//!
//! - [`build_deletion_draft`] / [`build_deletion_event`] — the canonical
//!   construction seam. All crates that need to publish a kind:5 event call
//!   this rather than hand-assembling tags.
//! - [`DeleteRecord`] / [`DeleteRecord::try_from_kernel_event`] — the generic
//!   read seam. Projections that ingest kind:5 events decode their `e`/`a`/`k`
//!   tags through this type rather than hand-parsing tags (#2589).
//! - [`AddressCoordinate`] — the canonical `kind:pubkey:d` address-coordinate
//!   identity for the `a`-tag grammar. Shared by every crate that reads or
//!   writes an addressable-event coordinate, not only deletion (#2589).
//! - [`register`] / [`DeleteModule`] — a generic "delete my event"
//!   action that apps can dispatch without writing any kind:5 wire code.
//! - [`ownership`] — the compiled ownership descriptor (ADR-0074).
//!
//! # What this crate does NOT own
//!
//! - Reaction semantics or viewer-reaction identity (`nmp-nip25`).
//! - Group envelopes, host relay pins, or `h`/`previous` tags (`nmp-nip29`).
//! - Publish signing, routing mechanics, or relay selection (`nmp-core`).
//! - Repost-target derivation or same-author-retracts-wrapper comparison
//!   logic — that stays caller-side (`nmp-nip18`, `nmp-content`,
//!   `nmp-note-feed`), which compares `DeleteRecord::author`/`created_at`
//!   against their own stored rows.
//! - App-private deletion policy beyond the generic NIP-09 construction rule.

mod action;
mod builder;
mod coordinate;
mod read;

pub use action::{DeleteAction, DeleteModule};
pub use builder::{
    build_deletion_draft, build_deletion_event, DeletionRequest, OwnedDeletionDraft,
};
pub use coordinate::{is_addressable_kind, AddressCoordinate};
pub use read::DeleteRecord;

/// NIP-09 deletion event kind.
pub const KIND_DELETION: u32 = 5;

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;

#[derive(Clone, Debug, Default)]
pub struct Config;

#[derive(Clone, Debug, Default)]
pub struct Handles;

pub fn register(
    app: &mut impl nmp_core::substrate::ActionRegistrar,
    _config: Config,
) -> Result<Handles, nmp_core::substrate::RegistrationError> {
    action::register_actions(app);
    Ok(Handles)
}
