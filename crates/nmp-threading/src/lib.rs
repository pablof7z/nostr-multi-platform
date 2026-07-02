//! `nmp-threading` — reply-convention-agnostic timeline grouping, plus the
//! reactive `nmp.threading.graph` e-tag threading read model.
//!
//! Owns the kind-agnostic [`Grouper`] that collapses reply chains into
//! Twitter-style stacked-module blocks, the trait surface ([`ParentResolver`])
//! and value types ([`ThreadPointer`], [`ModulePolicy`], [`TimelineBlock`],
//! [`GroupDelta`]) consumed by per-NIP wrapper view modules, AND
//! [`ThreadingProjection`] — a kind-blind [`nmp_core::ObservedProjectionSink`]
//! that folds a caller-supplied event scope into [`ThreadEdge`] rows plus the
//! same block grouping, for consumers that need a reactive read model instead
//! of (or in addition to) a per-view wrapper. Depends only on substrate
//! crates — no kind numbers, no tag literals, no app nouns.
//!
//! - `nmp-nip01::Nip10ModularTimelineView` wraps [`Grouper`] directly for
//!   NIP-10 kind:1.
//! - App-facing typed read sessions (e.g. `nmp-native-runtime`'s
//!   `NmpApp::open_nip29_group_threading_session`; see
//!   `docs/recipes/app-shapes.md`, "Group Timeline + Reply Chips") assemble
//!   [`ThreadingProjection`] against a relay-pinned event scope — this crate
//!   assembles no `ObservedProjection` and knows no relay filter itself.
//!
//! See `docs/decisions/0009-app-extension-kernel-boundary.md` (sibling-crate
//! packaging rule), `docs/decisions/0010-generated-app-enum-vs-type-
//! erased-registry.md`, `docs/decisions/0070-typed-read-sessions.md`, and
//! `docs/decisions/0076-app-facing-feed-helpers.md`.

pub mod block;
pub mod etag;
pub mod grouper;
pub mod pointer;
pub mod policy;
pub mod projection;
pub mod resolver;
pub mod wire;

pub use block::TimelineBlock;
pub use etag::EtagThreadResolver;
pub use grouper::{GroupDelta, Grouper};
pub use pointer::ThreadPointer;
pub use policy::ModulePolicy;
pub use projection::{ThreadEdge, ThreadingProjection, ThreadingSnapshot};
pub use resolver::ParentResolver;
pub use wire::{
    decode_threading_snapshot, encode_threading_snapshot, THREADING_GRAPH_FILE_IDENTIFIER,
    THREADING_GRAPH_SCHEMA_ID, THREADING_GRAPH_SCHEMA_VERSION,
};

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
