//! `nmp-replies` — the app-facing reply owner for NMP.
//!
//! Apps provide a [`ReplyTarget`] and content. This crate decides whether the
//! reply is a NIP-10 kind:1 note or a NIP-22 kind:1111 comment, and it compiles
//! the matching read plan for reply acquisition. Protocol-specific tag shape
//! stays in `nmp-nip01` / `nmp-nip22`; apps do not construct `#e`, `#E`, `#A`,
//! `#I`, NIP-10 markers, or NIP-22 scopes.

mod action;
mod build;
mod installer;
pub mod ownership;
mod read;
mod summary;
mod target;
mod wire;

pub use action::{ReplyAction, ReplyCommand, ReplyModule};
pub use build::{Reply, ReplyBuildError, ReplyBuilder};
pub use installer::{register, Config, Handles};
pub use read::{reply_read_plans, ReplyProtocol, ReplyReadMode, ReplyReadPlan, ReplyReadPlanError};
pub use summary::{
    close_replies, decode_reply_summary_snapshot, encode_reply_summary_snapshot, open_replies,
    RepliesReadHandle, ReplySummaryProjection, ReplySummarySnapshot, REPLY_SUMMARY_FILE_IDENTIFIER,
    REPLY_SUMMARY_SCHEMA_ID, REPLY_SUMMARY_SCHEMA_VERSION,
};
pub use target::{
    decode_and_validate_reply_target, ReplyAddressTarget, ReplyEventTarget, ReplyExternalTarget,
    ReplyTarget, ReplyTargetError, ReplyTargetParams, ReplyTargetParamsError,
};
