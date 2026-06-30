//! `nmp-replies` — the app-facing reply owner for NMP.
//!
//! Apps provide a [`ReplyTarget`] and content. This crate decides whether the
//! reply is a NIP-10 kind:1 note or a NIP-22 kind:1111 comment, and it compiles
//! the matching read plan for reply acquisition. Protocol-specific tag shape
//! stays in `nmp-nip01` / `nmp-nip22`; apps do not construct `#e`, `#E`, `#A`,
//! `#I`, NIP-10 markers, or NIP-22 scopes.

mod action;
mod build;
mod read;
mod target;
mod wire;

pub use action::{register_actions, ReplyAction, ReplyCommand, ReplyModule};
pub use build::{Reply, ReplyBuildError, ReplyBuilder};
pub use read::{ReplyProtocol, ReplyReadMode, ReplyReadPlan, ReplyReadPlanError};
pub use target::{
    ReplyAddressTarget, ReplyEventTarget, ReplyExternalTarget, ReplyTarget, ReplyTargetError,
};
