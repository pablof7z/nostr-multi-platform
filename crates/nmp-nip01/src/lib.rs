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
//! - [`kinds`] — `KIND_SHORT_TEXT_NOTE = 1`.
//! - [`decode`] — `NoteRecord` carrying [`Nip10Refs`] (parsed once at decode).
//! - [`nip10`] — NIP-10 reference parser and reply tag builder.
//! - [`build`] — `Note::new(content).reply_to(parent).build(author, ts)`
//!   producing an `UnsignedEvent` with NIP-10 marked tags via
//!   [`nmp_core::tags`].
//! - [`view`] — `RepliesView` (flat direct replies) + `ThreadView`
//!   (parent/child tree with out-of-order arrival buffering).
//! - [`meta_timeline`] — `Nip10ModularTimelineView` for protocol grouping
//!   over `nmp_threading::Grouper`.

mod timeline_snapshot_generated {
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

pub mod build;
pub mod decode;
pub mod draft;
pub mod kind0_parser;
pub mod kinds;
pub mod meta_timeline;
pub mod nip10;
pub mod profile_cache;
mod profile_display;
pub mod timeline_projection;
pub mod typed_wire;
pub mod view;

pub use build::{Note, NoteBuildError, NoteBuilder};
pub use decode::{try_from_event, try_from_kernel_event, NoteRecord};
pub use draft::register_draft_builders;
pub use kind0_parser::Kind0Parser;
pub use kinds::KIND_SHORT_TEXT_NOTE;
pub use meta_timeline::{
    ModularTimelineDelta, ModularTimelinePayload, ModularTimelineSpec, ModularTimelineState,
    Nip10ModularTimelineView, Nip10Resolver,
};
pub use nip10::{parse_nip10, reply_tags, EventRef, Nip10Refs};
pub use profile_cache::ProfileCache;
pub use profile_display::{
    profile_metadata_projection_from_event, AuthorDisplay, ProfileDisplay,
    ProfileMetadataProjection,
};
pub use timeline_projection::{ModularTimelineProjection, ModularTimelineSnapshot};
pub use typed_wire::{
    decode_modular_timeline_snapshot, encode_modular_timeline_snapshot,
    FILE_IDENTIFIER as TIMELINE_SNAPSHOT_FILE_IDENTIFIER, SCHEMA_ID as TIMELINE_SNAPSHOT_SCHEMA_ID,
    SCHEMA_VERSION as TIMELINE_SNAPSHOT_SCHEMA_VERSION,
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
// `ObservedProjectionSink` — see `nmp_core::substrate` module docs.

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
