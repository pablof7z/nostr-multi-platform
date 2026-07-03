//! `nmp-reactions` — the app-facing reaction-count read owner for NMP (#2758).
//!
//! Apps call [`open_reactions`] with a raw target event id, or
//! [`open_nip25_group_reactions_session`] for a NIP-29-group-scoped aggregate.
//! This crate composes the NIP-25 kind:7/kind:5 fold (`nmp_nip25::
//! ReactionAggregateProjection`) into routed read demands and drives them
//! through the concept-neutral read-lifecycle engine (`nmp-read-session`).
//! Protocol semantics (reaction construction, content normalization,
//! retraction matching) stay owned by `nmp-nip25`; group routing stays owned by
//! `nmp-nip29`. This crate only declares the read composition.

mod group;
pub mod ownership;
mod read;
mod summary;
mod target;

pub use group::{
    close_nip25_group_reactions_session, group_reactions_filter_json,
    open_nip25_group_reactions_session, open_nip25_group_reactions_session_with_reader,
    Nip25GroupReactionsHandle, Nip25GroupReactionsSession, GROUP_REACTIONS_KEY,
};
pub use read::reaction_filter_json;
pub use summary::{
    close_reactions, decode_reaction_summary_snapshot, encode_reaction_summary_snapshot,
    open_reactions, ReactionGroupSummary, ReactionSummarySnapshot, ReactionsReadHandle,
    REACTION_SUMMARY_FILE_IDENTIFIER, REACTION_SUMMARY_SCHEMA_ID, REACTION_SUMMARY_SCHEMA_VERSION,
};
pub use target::{ReactionTarget, ReactionTargetError};
