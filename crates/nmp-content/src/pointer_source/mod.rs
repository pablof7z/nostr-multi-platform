//! Pointer-source target-hydration read model (#2113).
//!
//! The NMP read-model equivalent of NDK `$metaSubscribe`. A *pointer source* is
//! one ordinary tailing interest whose events (kind:6 reposts, kind:7 reactions,
//! kind:9802 highlights, kind:1111 comments, …) carry `e` / `a` tag references to
//! the *real* content. This module owns the read-model half of that pattern:
//!
//! * extract `e` (event-id) and `a` (address-coordinate) references from each
//!   pointer event into a demanded [`EmbedTarget`](crate::EmbedTarget) set;
//! * maintain the bidirectional `pointedBy` reverse index
//!   (`target -> pointer events`);
//! * hydrate targets from accepted target events (newest-wins for addressables);
//! * project the resolved targets in one of several [`PointerSortMode`]s, with
//!   re-sort that never touches the underlying interests.
//!
//! # What lives here vs. the kernel
//!
//! This module owns only the pure read-model state — extracting demanded
//! targets from pointer events, hydrating them, and projecting the result.
//! Wiring that state to the kernel's observed-projection and
//! dependent-interest seams is the read engine's job now
//! (`DependentDemandReconciler`, `crates/nmp-read-session/src/dependent.rs`,
//! #2818); the hand-rolled composition root that used to live here was
//! deleted as dead code (#2956).
//!
//! See `docs/research/ndk/meta-subscribe.md` for the full NDK mapping and the
//! relationship to the adjacent ReducedSource feed primitive (#2092).

mod model;
mod projection;
#[cfg(test)]
mod tests;

pub use model::PointerSourceModel;
pub use projection::{PointerItem, PointerSortMode};
