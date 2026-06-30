//! `nmp-nip09` — NIP-09 generic deletion (kind:5) artifact ownership for NMP.
//!
//! This crate is the **exclusive positive owner** of the NIP-09 kind:5
//! deletion wire grammar. It provides:
//!
//! - [`build_deletion_draft`] / [`build_deletion_event`] — the canonical
//!   construction seam. All crates that need to publish a kind:5 event call
//!   this rather than hand-assembling tags.
//! - [`deletion_targets`] — the generic read seam. Projections that ingest
//!   kind:5 events parse their `e`/`k` tags through this function.
//! - [`Nip09Descriptor`] / [`DeleteModule`] — a generic "delete my event"
//!   action that apps can dispatch without writing any kind:5 wire code.
//! - [`ownership`] — the compiled ownership descriptor (ADR-0074).
//!
//! # What this crate does NOT own
//!
//! - Reaction semantics or viewer-reaction identity (`nmp-nip25`).
//! - Group envelopes, host relay pins, or `h`/`previous` tags (`nmp-nip29`).
//! - Publish signing, routing mechanics, or relay selection (`nmp-core`).
//! - App-private deletion policy beyond the generic NIP-09 construction rule.

mod action;
mod builder;
mod read;

pub use action::{DeleteAction, DeleteModule, Nip09Descriptor};
pub use builder::{build_deletion_draft, build_deletion_event, DeletionRequest, OwnedDeletionDraft};
pub use read::{deletion_targets, DeletionTargets};

/// NIP-09 deletion event kind.
pub const KIND_DELETION: u32 = 5;

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
