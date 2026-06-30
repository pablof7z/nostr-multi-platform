//! `nmp-nip22` — NIP-22 comments (kind:1111) for NMP apps.
//!
//! This crate owns the NIP-22 comment surface:
//!
//! - [`builder`] — pure kind:1111 comment event construction for protocol
//!   owners. App-facing reply policy lives in `nmp-replies`; apps should not
//!   construct `A`/`E`/`I` scopes directly.
//! - [`decode`] — raw [`CommentRecord`] decode from a kernel event, parsing
//!   the UPPERCASE root scope (`A`/`E`/`I` + `K`) and lowercase parent scope
//!   (`a`/`e`/`i` + `k`). Top-level comments mirror the root.
//! - [`projection`] — [`CommentThreadProjection`], an in-memory
//!   `ObservedProjectionSink` (same shape as `nmp-nip25` reactions and
//!   `nmp-nip51` bookmarks) that buckets kind:1111 by root and builds the
//!   parent/child forest for a root on demand.
//!
//! Display formatting (counts, labels, symbols) belongs in the shell, not here
//! (D1).

mod builder;
mod decode;
mod projection;
pub mod runtime;

pub use builder::{
    build_comment_event, CommentBuildError, CommentBuildInput, CommentParent, CommentRoot,
};
pub use decode::{try_from_kernel_event, CommentRecord};
pub use nmp_kinds::KIND_NIP22_COMMENT;
pub use projection::{
    build_thread, CommentThreadNode, CommentThreadProjection, CommentThreadSnapshot,
};
pub use runtime::register_comment_runtime;

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
