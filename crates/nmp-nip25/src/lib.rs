//! `nmp-nip25` — public NIP-25 reaction ownership for NMP.
//!
//! This crate owns the public kind:7 reaction action surface and the bounded
//! viewer-reaction read model. In-group reactions with an `h` tag remain owned
//! by `nmp-nip29`; neutral timeline relation counters may still display kind:7
//! rows without importing this crate.

mod action;
mod projection;

pub use action::{
    register_actions, PublishReactionCommand, ReactAction, ReactModule, UnreactAction,
    UnreactModule, UnreactReactionCommand, KIND_REACTION, KIND_REACTION_DELETE,
};
pub use projection::{ReactionEntry, ReactionProjection, ReactionSnapshot, ViewerReactionState};
