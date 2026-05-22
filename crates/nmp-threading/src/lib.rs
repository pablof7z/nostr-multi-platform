//! `nmp-threading` — reply-convention-agnostic timeline grouping.
//!
//! Owns the kind-agnostic [`Grouper`] that collapses reply chains into
//! Twitter-style stacked-module blocks, plus the trait surface
//! ([`ParentResolver`]) and value types ([`ThreadPointer`], [`ModulePolicy`],
//! [`TimelineBlock`], [`GroupDelta`]) consumed by per-NIP wrapper view
//! modules. Depends only on `nmp-core` — no kind numbers, no tag literals,
//! no app nouns.
//!
//! - `nmp-nip01::Nip10ModularTimelineView` wraps this for NIP-10 kind:1.
//!
//! See `docs/decisions/0009-app-extension-kernel-boundary.md` (sibling-crate
//! packaging rule) and `docs/decisions/0010-generated-app-enum-vs-type-
//! erased-registry.md`.

pub mod block;
pub mod grouper;
pub mod policy;
pub mod pointer;
pub mod resolver;

pub use block::TimelineBlock;
pub use grouper::{GroupDelta, Grouper};
pub use policy::ModulePolicy;
pub use pointer::ThreadPointer;
pub use resolver::ParentResolver;
