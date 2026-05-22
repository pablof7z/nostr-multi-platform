//! `nmp-nip01` — NIP-01 short text notes (kind:1) relation surface as an NMP
//! protocol crate.
//!
//! Implements the design recommendation in `docs/design/kind-wrappers.md` §3
//! restricted to the **relation read-views + note/reply builder** scope.
//! Extracting the kernel's existing kind-1 timeline ingest into `nmp-nip01`
//! is a separate doctrine effort (kind-wrappers.md Phase 1 §8) and
//! intentionally out of scope here.
//!
//! ## Module layout
//!
//! - [`kinds`] — `KIND_SHORT_NOTE = 1`.
//! - [`decode`] — `NoteRecord` carrying `Nip10Refs` (parsed once at decode).
//! - [`build`] — `Note::new(content).reply_to(parent).build(author, ts)`
//!   producing an `UnsignedEvent` with NIP-10 marked tags via
//!   [`nmp_core::tags`].
//! - [`view`] — `RepliesView` (flat direct replies) + `ThreadView`
//!   (parent/child tree with out-of-order arrival buffering).
//! - [`meta_timeline`] — `Nip10ModularTimelineView` (Twitter-style
//!   stacked-modules timeline; wraps `nmp_threading::Grouper`).

pub mod build;
pub mod decode;
pub mod kinds;
pub mod meta_timeline;
mod note_relations;
mod profile_display;
pub mod timeline_projection;
pub mod view;

pub use build::{Note, NoteBuildError, NoteBuilder};
pub use decode::{try_from_event, try_from_kernel_event, NoteRecord};
pub use kinds::KIND_SHORT_NOTE;
pub use meta_timeline::{
    ModularTimelineDelta, ModularTimelinePayload, ModularTimelineSpec, ModularTimelineState,
    Nip10ModularTimelineView, Nip10Resolver,
};
pub use note_relations::{NoteRelationCounts, RelationCount, RelationCountInterest};
pub use profile_display::{AuthorDisplay, AuthorDisplaySource};
pub use timeline_projection::{
    ModularTimelineProjection, ModularTimelineSnapshot, TimelineEventCard,
};
pub use view::{
    RepliesDelta, RepliesPayload, RepliesSpec, RepliesState, RepliesView, ThreadDelta, ThreadNode,
    ThreadPayload, ThreadSpec, ThreadState, ThreadView,
};

// NOTE: `nmp-nip01` exposes its view types (`RepliesView`, `ThreadView`,
// `Nip10ModularTimelineView`) as plain public types whose `open` /
// `on_event_*` / `snapshot` inherent methods are reached via static
// dispatch — the `ViewModule` trait and the former
// `register(&mut ModuleRegistry)` entry point were both deleted because no
// kernel-side registry ever drove them. The live extension path is
// `KernelEventObserver` — see `nmp_core::substrate` module docs.
