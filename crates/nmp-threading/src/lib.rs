//! `nmp-threading` — reply-convention-agnostic timeline grouping.
//!
//! Owns the kind-agnostic [`Grouper`] that collapses reply chains into
//! Twitter-style stacked-module blocks, plus the trait surface
//! ([`ParentResolver`]) and value types ([`ThreadPointer`], [`ModulePolicy`],
//! [`TimelineBlock`], [`GroupDelta`]) consumed by per-NIP wrapper view
//! modules. It also owns the generic e-tag threading projection family
//! (`nmp.threading.graph.*`) for consumers that need a reactive read model over
//! a caller-supplied event scope. Depends only on substrate crates — no kind
//! numbers, no app nouns.
//!
//! - `nmp-nip01::Nip10ModularTimelineView` wraps this for NIP-10 kind:1.
//!
//! See `docs/decisions/0072-runtime-capability-and-shell-boundary.md` and
//! `docs/architecture/crate-boundaries.md` for the sibling-crate packaging
//! rule.

pub mod block;
pub mod etag;
pub mod grouper;
pub mod pointer;
pub mod policy;
pub mod projection;
pub mod resolver;
pub mod runtime;
pub mod wire;

pub use block::TimelineBlock;
pub use etag::EtagThreadResolver;
pub use grouper::{GroupDelta, Grouper};
pub use pointer::ThreadPointer;
pub use policy::ModulePolicy;
pub use projection::{ThreadEdge, ThreadingProjection, ThreadingSnapshot};
pub use resolver::ParentResolver;
pub use runtime::{
    close_threading_read_model, open_threading_read_model, threading_projection_key,
    ThreadingReadModelHandle, ThreadingReadModelParams, ThreadingScope,
    THREADING_GRAPH_PROJECTION_FAMILY_CLAIM, THREADING_GRAPH_PROJECTION_KEY_PREFIX,
    THREADING_GRAPH_SCHEMA_ID, THREADING_GRAPH_SESSION_ID_MAX_LEN,
};
pub use wire::{
    decode_threading_snapshot, encode_threading_snapshot, THREADING_GRAPH_FILE_IDENTIFIER,
    THREADING_GRAPH_SCHEMA_VERSION,
};

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
