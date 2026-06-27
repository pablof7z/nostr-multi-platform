//! `nmp-nip22` — NIP-22 comments (kind:1111) for NMP apps.
//!
//! This crate owns the NIP-22 comment surface:
//!
//! - [`decode`] — raw [`CommentRecord`] decode from a kernel event, parsing
//!   the UPPERCASE root scope (`A`/`E`/`I` + `K`) and lowercase parent scope
//!   (`a`/`e`/`i` + `k`). Top-level comments mirror the root.
//! - [`projection`] — [`CommentThreadProjection`], an in-memory
//!   `ObservedProjectionSink` (same shape as `nmp-nip25` reactions and
//!   `nmp-nip51` bookmarks) that buckets kind:1111 by root and builds the
//!   parent/child forest for a root on demand.
//! - [`action`] — the `nmp.nip22.post_comment` action module that builds a
//!   correctly-scoped kind:1111 event.
//!
//! Comment-count aggregation per root lives in `nmp-nip01`'s `note_relations`
//! (alongside reaction/repost/zap counts); this crate only declares the raw
//! comment surface. Display formatting (counts, labels, symbols) belongs in
//! the shell, not here (D1).

mod action;
mod decode;
mod projection;
// ADR-0064 / S9 (#1747) — typed FlatBuffers payload codec (`ActionPayload`
// impl for `PostCommentAction`).
mod wire;

pub use action::{register_actions, PostCommentAction, PostCommentCommand, PostCommentModule};
pub use nmp_kinds::KIND_NIP22_COMMENT;
pub use decode::{try_from_kernel_event, CommentRecord};
pub use projection::{
    build_thread, CommentThreadNode, CommentThreadProjection, CommentThreadSnapshot,
};
