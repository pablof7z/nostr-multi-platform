//! `nmp-nip25` — public NIP-25 reaction ownership for NMP.
//!
//! This crate owns the public kind:7 reaction action surface, the bounded
//! viewer-reaction read model, and the kind:7 **aggregation** read model
//! (counts + per-emoji breakdown + reactor pubkeys, folded by target id). The
//! aggregation is kind:7-scoped but group-agnostic: scoping the fold to one
//! NIP-29 group is composed at the app layer by the feeding `#h` + `kinds:[7]`
//! interest filter, so this crate never names the `h` tag (it does not depend
//! on `nmp-nip29`). Neutral timeline relation counters may still display kind:7
//! rows without importing this crate.

mod action;
// NIP-25 reaction **aggregation** read model (kind:7 folded by target id) — the
// reusable group-scopable seam a chat / thread UI reads for reaction chips +
// counts + reactor pubkeys.
mod aggregate;
mod projection;
// ADR-0064 / S3 (#1751) — typed FlatBuffers payload codecs (`ActionPayload`
// impls for `ReactAction` / `UnreactAction`) PLUS the read-side
// reaction-aggregate sidecar codec (`reaction_aggregate_fb`).
mod wire;

pub use action::{
    Nip25Descriptor, PublishReactionCommand, ReactAction, ReactModule,
    UnreactAction, UnreactModule, UnreactReactionCommand, KIND_REACTION, KIND_REACTION_DELETE,
};
pub use aggregate::{
    ReactionAggregateProjection, ReactionAggregateSnapshot, ReactionEmojiCount,
    ReactionTargetAggregate,
};
pub use projection::{ReactionEntry, ReactionProjection, ReactionSnapshot, ViewerReactionState};
pub use wire::reaction_aggregate_fb::{
    decode_reaction_aggregate_snapshot, encode_reaction_aggregate_snapshot,
    REACTION_AGGREGATE_FILE_IDENTIFIER, REACTION_AGGREGATE_SCHEMA_ID,
    REACTION_AGGREGATE_SCHEMA_VERSION,
};
