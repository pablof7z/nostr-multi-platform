//! `nmp-nip22` — NIP-22 standalone comments (kind 1111, non-NIP-29 form) as
//! an NMP protocol crate.
//!
//! Owns kind 1111 events that do **not** carry an `h` tag. Kind 1111 events
//! with `h` belong to `nmp-nip29` (group comments) per the `(kind, h-tag)`
//! D4 discriminator from `kind-wrappers.md` §6.
//!
//! Implements the design recommendation in §3: pure decoder → immutable
//! [`CommentRecord`] (with `CommentPointer` root + parent), consume-self
//! [`CommentBuilder`], `CommentsView` reactive read projection, and a
//! reverse-index for `(parent_event_id → comment_ids)` (see [`domain`]).

pub mod build;
pub mod decode;
pub mod domain;
pub mod kinds;
pub mod meta_timeline;
pub mod view;

pub use build::{Comment, CommentBuildError, CommentBuilder};
pub use decode::{try_from_event, try_from_kernel_event, CommentPointer, CommentRecord};
pub use domain::{decode_and_route, list_by_parent, NAMESPACE};
pub use kinds::KIND_COMMENT;
pub use meta_timeline::{
    ModularTimelineDelta, ModularTimelinePayload, ModularTimelineSpec, ModularTimelineState,
    Nip22ModularTimelineView, Nip22Resolver,
};
pub use view::{CommentsDelta, CommentsPayload, CommentsSpec, CommentsState, CommentsView};

// NOTE: `nmp-nip22` exposes its view types (`CommentsView`,
// `Nip22ModularTimelineView`) as plain public types whose `open` /
// `on_event_*` / `snapshot` inherent methods are reached via static
// dispatch — the `ViewModule` trait and the former
// `register(&mut ModuleRegistry)` entry point were both deleted because no
// kernel-side registry ever drove them. The live extension path is
// `KernelEventObserver` — see `nmp_core::substrate` module docs.
