//! `Grouper<R>` — kind-agnostic timeline grouping. Given a stream of
//! `KernelEvent`s and a `ParentResolver`, emits a `Vec<TimelineBlock>` where
//! reply chains collapse into Twitter-style modules.
//!
//! ## Algorithm sketch
//!
//! On each event insert:
//!   1. Ignore if already known.
//!   2. Resolve parent via the per-NIP `ParentResolver`.
//!   3. If parent is an `Event` already in store AND occupies the leaf of an
//!      existing block, splice the new event onto that block (promoting
//!      Standalone → Module if needed) up to `policy.max_module_size`.
//!   4. Otherwise walk ancestors up to `policy.max_ancestor_hops`, picking
//!      up `Event` ids that are in the store and not yet `seen`. `Address`
//!      / `External` parents terminate the walk and become the module's
//!      `root` pointer.
//!   5. Wrap the chain in a `TimelineBlock`; `blocks` is kept sorted by
//!      newest event timestamp, regardless of relay arrival order.
//!   6. If the parent is unknown locally, buffer the child in `orphans`
//!      keyed by the missing parent id. Parent arrival replays children.
//!
//! Adjacent-block collapse runs after every mutation: two `Module` blocks
//! sharing the same `root` pointer merge if `policy.collapse_adjacent_same_
//! root` is set and the merged length would fit `max_module_size`.
//!
//! ## Why no dynamic dependency injection
//!
//! A view's `dependencies` is a pure function of its spec. There is no API
//! to re-publish dependencies with `pending_ancestor_ids` learned at
//! runtime. `ThreadView` lives with the same constraint and relies on the
//! surrounding planner subscription (broad `("e", target)` tag-ref) to
//! surface ancestors. Wrappers around this grouper inherit that contract;
//! `pending_ancestor_ids` is kept as internal diagnostic state.
//!
//! ## Module layout
//!
//! The algorithm is split by phase across submodules; this file is the
//! spine (state + construction + read accessors) that the phases share:
//!   - [`lifecycle`] — insert/remove/replace entry points, orphan replay,
//!     supersession bookkeeping.
//!   - [`placement`] — splicing an event onto an existing block or walking
//!     ancestors to build a fresh chain.
//!   - [`ordering`] — block recency ordering and delta-index resolution.
//!   - [`collapse`] — adjacent same-root module merging.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use nmp_core::substrate::{EventId, KernelEvent};
use serde::{Deserialize, Serialize};

use crate::block::TimelineBlock;
use crate::policy::ModulePolicy;
use crate::resolver::ParentResolver;

mod collapse;
mod lifecycle;
mod ordering;
mod placement;

/// Delta surface for the grouper. Wrappers map this into their own
/// view-module `Delta` type (typically a 1:1 forward).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum GroupDelta {
    /// A new block was inserted at the given display index.
    BlockInserted(usize),
    /// A block at the given index was replaced (length / membership /
    /// `has_gap` changed). Wrappers re-emit the full block from
    /// [`Grouper::blocks`].
    BlockReplaced(usize),
    /// A block at the given index was removed.
    BlockRemoved(usize),
}

/// Owning state for the algorithm. One instance per open view.
pub struct Grouper<R: ParentResolver> {
    resolver: R,
    policy: ModulePolicy,
    /// Display-order blocks: index 0 is newest.
    blocks: Vec<TimelineBlock>,
    /// Every event id the grouper has accepted into some block.
    seen: HashSet<EventId>,
    /// Full event payloads we have observed (parent lookups + replay).
    by_id: BTreeMap<EventId, KernelEvent>,
    /// Children waiting on a parent id. Replayed on parent arrival.
    orphans: BTreeMap<EventId, BTreeSet<EventId>>,
    /// Events currently buffered as orphans (their own parent is still
    /// unknown). They must NOT be absorbed by another event's ancestor walk
    /// — when their parent later arrives we want a clean stitch, not a
    /// half-attached chain that needs re-stitching.
    orphaned: HashSet<EventId>,
    /// Ancestor event ids the grouper would like the planner to surface —
    /// declared but the substrate has no dynamic-deps API yet. Kept for
    /// diagnostics / a future trait extension.
    pending_ancestor_ids: BTreeSet<EventId>,
    /// Per-target set of superseding event ids. While a target's set is
    /// non-empty, its standalone block is suppressed from the layout — a
    /// late-arriving target won't get its own block either. The target stays
    /// in `by_id` so reply chains can still locate it as a parent, and so the
    /// block can be restored if all its superseders are later removed.
    ///
    /// Populated by `ParentResolver::supersedes` (e.g., a NIP-18 repost names
    /// the note it should bump in the feed).
    superseded_by: BTreeMap<EventId, BTreeSet<EventId>>,
}

impl<R: ParentResolver> Grouper<R> {
    #[must_use]
    pub fn new(resolver: R, policy: ModulePolicy) -> Self {
        Self {
            resolver,
            policy,
            blocks: Vec::new(),
            seen: HashSet::new(),
            by_id: BTreeMap::new(),
            orphans: BTreeMap::new(),
            orphaned: HashSet::new(),
            pending_ancestor_ids: BTreeSet::new(),
            superseded_by: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn blocks(&self) -> &[TimelineBlock] {
        &self.blocks
    }

    #[must_use]
    pub fn pending_ancestor_ids(&self) -> &BTreeSet<EventId> {
        &self.pending_ancestor_ids
    }

    #[must_use]
    pub fn event(&self, id: &EventId) -> Option<&KernelEvent> {
        self.by_id.get(id)
    }
}
