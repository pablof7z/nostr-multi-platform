//! `nmp-reactions` — the app-facing reaction-count read owner for NMP (#2758).
//!
//! Apps call [`open_reactions`] with a raw target event id. This crate
//! composes the NIP-25 kind:7/kind:5 fold (`nmp_nip25::
//! ReactionAggregateProjection`) into ONE routed demand and drives it through
//! the concept-neutral read-lifecycle engine (`nmp-read-session`). Protocol
//! semantics (reaction construction, content normalization, retraction
//! matching) stay owned by `nmp-nip25`; this crate never reimplements them —
//! it is a thin declarative concept spec over the existing adapter, exactly
//! like `nmp-replies` is over `nmp-nip01`/`nmp-nip22`.

pub mod ownership;
mod read;
mod summary;
mod target;

pub use read::reaction_filter_json;
pub use summary::{
    close_reactions, encode_reaction_summary_snapshot, open_reactions, ReactionGroupSummary,
    ReactionSummarySnapshot, ReactionsReadHandle, REACTION_SUMMARY_FILE_IDENTIFIER,
    REACTION_SUMMARY_SCHEMA_ID, REACTION_SUMMARY_SCHEMA_VERSION,
};
pub use target::{ReactionTarget, ReactionTargetError};
