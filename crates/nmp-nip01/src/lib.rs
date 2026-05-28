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

// FlatBuffers-generated bindings, mounted at the crate root. The OP-feed schema
// (`op_feed.fbs`) `include`s the timeline schema and references its
// `TimelineEventCard` / `AuthorDisplay` tables; `flatc` (no `--gen-all`) emits a
// crate-root `use crate::timeline_snapshot_generated::*;` into
// `op_feed_generated.rs`. That glob only sees items at the *top* of
// `timeline_snapshot_generated`, but the generated leaf types are nested under
// `nmp::nip_01`. So the timeline module is wrapped to flat-re-export its leaf
// types at the module root: the sibling generated file's glob then resolves
// `TimelineEventCard` / `AuthorDisplay` by short name. The wrapper hides the
// generated `pub mod nmp` inside `inner` so it does not collide with
// `op_feed_generated.rs`'s own `pub mod nmp` declaration.
mod timeline_snapshot_generated {
    mod inner {
        #![allow(
            clippy::all,
            dead_code,
            deprecated,
            missing_docs,
            non_camel_case_types,
            non_snake_case,
            unused_imports
        )]
        include!("wire/generated/timeline_snapshot_generated.rs");
    }
    pub use inner::nmp::nip_01::*;
}
#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unused_imports
)]
#[path = "wire/generated/op_feed_generated.rs"]
mod op_feed_generated;

pub mod build;
pub mod decode;
pub mod kinds;
pub mod meta_timeline;
mod note_relations;
pub mod op_feed;
mod profile_display;
pub mod timeline_projection;
pub mod typed_wire;
pub mod view;
pub mod visible_relations;

pub use build::{Note, NoteBuildError, NoteBuilder};
pub use decode::{try_from_event, try_from_kernel_event, NoteRecord};
pub use kinds::KIND_SHORT_NOTE;
pub use meta_timeline::{
    ModularTimelineDelta, ModularTimelinePayload, ModularTimelineSpec, ModularTimelineState,
    Nip10ModularTimelineView, Nip10Resolver,
};
pub use note_relations::{NoteRelationCounts, RelationCount, RelationCountInterest};
pub use op_feed::{
    decode_op_feed_snapshot, encode_op_feed_snapshot, register_op_feed, Nip10ReplyAttribution,
    OpFeedEngine, OpFeedSnapshot, OP_FEED_FILE_IDENTIFIER, OP_FEED_SCHEMA_ID,
    OP_FEED_SCHEMA_VERSION,
};
pub use profile_display::{AuthorDisplay, ProfileDisplay};
pub use timeline_projection::{
    ModularTimelineProjection, ModularTimelineSnapshot, TimelineEventCard, TimelineWindowCursor,
    TimelineWindowMetrics, TimelineWindowPage, TimelineWindowRequest,
    DEFAULT_TIMELINE_WINDOW_LIMIT, MAX_TIMELINE_WINDOW_LIMIT,
};
pub use typed_wire::{
    decode_modular_timeline_snapshot, encode_modular_timeline_snapshot,
    FILE_IDENTIFIER as TIMELINE_SNAPSHOT_FILE_IDENTIFIER, SCHEMA_ID as TIMELINE_SNAPSHOT_SCHEMA_ID,
    SCHEMA_VERSION as TIMELINE_SNAPSHOT_SCHEMA_VERSION,
};
pub use view::{
    RepliesDelta, RepliesPayload, RepliesSpec, RepliesState, RepliesView, ThreadDelta, ThreadNode,
    ThreadPayload, ThreadSpec, ThreadState, ThreadView,
};
pub use visible_relations::{
    register_visible_note_relation_actions, visible_note_relations_identity,
    visible_note_relations_interest, visible_note_relations_interest_id,
    VisibleNoteRelationsAction, VisibleNoteRelationsModule, VISIBLE_NOTE_RELATIONS_LIMIT,
    VISIBLE_NOTE_RELATIONS_NAMESPACE,
};

// NOTE: `nmp-nip01` exposes its view types (`RepliesView`, `ThreadView`,
// `Nip10ModularTimelineView`) as plain public types whose `open` /
// `on_event_*` / `snapshot` inherent methods are reached via static
// dispatch — the `ViewModule` trait and the former
// `register(&mut ModuleRegistry)` entry point were both deleted because no
// kernel-side registry ever drove them. The live extension path is
// `KernelEventObserver` — see `nmp_core::substrate` module docs.
