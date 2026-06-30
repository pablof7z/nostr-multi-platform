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
//! [`register_pointer_source`] drives the read model through the kernel's
//! observed-projection and dependent-interest seams: it feeds pointer events in,
//! reads [`PointerSourceModel::target_demand`] back out, and materializes that
//! demand as a kernel-owned dependent-interest set plus a delivery projection.
//! Every target request therefore flows through the existing registry / planner
//! / router / cache path — there is no out-of-band `fetchEvents` lane.
//!
//! The `pointedBy` index is read-model state, not kernel source-of-truth state
//! (D4): the kernel owns the live target interests; this struct owns only the
//! derived projection.
//!
//! See `docs/research/ndk/meta-subscribe.md` for the full NDK mapping and the
//! relationship to the adjacent ReducedSource feed primitive (#2092).

mod composition;
#[cfg(test)]
mod composition_tests;
mod model;
mod projection;
mod shapes;
#[cfg(test)]
mod tests;

pub use composition::{
    open_pointer_source, register_pointer_source, PointerSourceParams, PointerSourceSession,
};
pub use model::PointerSourceModel;
pub use projection::{PointerItem, PointerSortMode};
