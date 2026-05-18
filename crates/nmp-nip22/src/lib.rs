//! `nmp-nip22` — NIP-22 standalone comments (kind 1111, non-NIP-29 form) as
//! an NMP protocol crate.
//!
//! Owns kind 1111 events that do **not** carry an `h` tag. Kind 1111 events
//! with `h` belong to `nmp-nip29` (group comments) per the `(kind, h-tag)`
//! D4 discriminator from `kind-wrappers.md` §6.
//!
//! Implements the design recommendation in §3: pure decoder → immutable
//! [`CommentRecord`] (with `CommentPointer` root + parent), consume-self
//! [`CommentBuilder`], `CommentsView` reactive read projection, and
//! `CommentsDomain` reverse-index for `(parent_event_id → comment_ids)`.

pub mod build;
pub mod decode;
pub mod domain;
pub mod kinds;
pub mod view;

pub use build::{Comment, CommentBuildError, CommentBuilder};
pub use decode::{try_from_event, try_from_kernel_event, CommentPointer, CommentRecord};
pub use domain::{decode_and_route, list_by_parent, CommentsDomain, NAMESPACE};
pub use kinds::KIND_COMMENT;
pub use view::{CommentsDelta, CommentsPayload, CommentsSpec, CommentsState, CommentsView};

use nmp_core::substrate::ModuleRegistry;

/// Register every module produced by `nmp-nip22` into a kernel
/// `ModuleRegistry`. Called by per-app generated code (`nmp-codegen`).
pub fn register(registry: &mut ModuleRegistry) {
    registry.register_domain::<CommentsDomain>();
    registry.register_view::<CommentsView>();
}
