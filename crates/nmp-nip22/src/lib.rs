//! `nmp-nip22` — NIP-22 standalone comments (kind 1111, non-NIP-29 form) as
//! an NMP protocol crate.
//!
//! Owns kind 1111 events that do **not** carry an `h` tag. Kind 1111 events
//! with `h` belong to `nmp-nip29` (group comments) per the `(kind, h-tag)`
//! D4 discriminator from `kind-wrappers.md` §6.
//!
//! Implements the design recommendation in §3: pure decoder → immutable
//! [`CommentRecord`] (with `CommentPointer` root + parent), consume-self
//! [`CommentBuilder`], and a `CommentsView` reactive read projection.

pub mod build;
pub mod decode;
pub mod kinds;
pub mod view;

pub use build::{Comment, CommentBuildError, CommentBuilder};
pub use decode::{try_from_event, try_from_kernel_event, CommentPointer, CommentRecord};
pub use kinds::KIND_COMMENT;
pub use view::{CommentsDelta, CommentsPayload, CommentsSpec, CommentsState, CommentsView};

// `nmp-nip22` exposes `CommentsView` as a plain public type whose `open` /
// `on_event_*` / `snapshot` inherent methods are reached via static dispatch.
// The live extension path is `KernelEventObserver` — see `nmp_core::substrate` docs.
