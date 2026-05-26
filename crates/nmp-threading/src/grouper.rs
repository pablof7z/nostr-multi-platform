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

use std::collections::{BTreeMap, BTreeSet, HashSet};

use nmp_core::substrate::{EventId, KernelEvent};
use serde::{Deserialize, Serialize};

use crate::block::TimelineBlock;
use crate::pointer::ThreadPointer;
use crate::policy::ModulePolicy;
use crate::resolver::ParentResolver;

mod collapse;

use self::collapse::{gap_between, root_id_mismatched};

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

    /// Process an inserted event. Returns the strongest single delta
    /// (wrappers re-snapshot anyway).
    #[must_use]
    pub fn on_insert(&mut self, event: &KernelEvent) -> Option<GroupDelta> {
        if self.by_id.contains_key(&event.id) {
            return None;
        }
        // Suppress events that have been preempted by a superseder. We still
        // record the payload in `by_id` so reply chains and ancestor walks can
        // resolve this id as a parent — only the standalone block placement
        // is skipped.
        if self
            .superseded_by
            .get(&event.id)
            .is_some_and(|set| !set.is_empty())
        {
            self.by_id.insert(event.id.clone(), event.clone());
            return None;
        }
        self.by_id.insert(event.id.clone(), event.clone());

        // Supersession: if this event supersedes a target (e.g., a repost
        // bumping the original note), evict the target's standalone block so
        // the superseder takes its place in the layout. Reply chains that
        // contain the target are left untouched — the target is still useful
        // as parent context.
        if let Some(target) = self.resolver.supersedes(event) {
            self.superseded_by
                .entry(target.clone())
                .or_default()
                .insert(event.id.clone());
            self.unplace_standalone(&target);
        }

        // Drain any orphans waiting on this event's id; they will replay
        // after we've placed this event itself.
        let waiting = self.orphans.remove(&event.id).unwrap_or_default();

        let mut delta = self.place_event(event);

        // Replay waiting children. Each replay may release further orphans.
        let mut replay_queue: Vec<EventId> = waiting.into_iter().collect();
        while let Some(child_id) = replay_queue.pop() {
            if self.seen.contains(&child_id) {
                continue;
            }
            let Some(child) = self.by_id.get(&child_id).cloned() else {
                continue;
            };
            self.place_event(&child);
            if let Some(more) = self.orphans.remove(&child_id) {
                replay_queue.extend(more);
            }
        }

        self.sort_blocks_newest_first();
        self.collapse_adjacent();
        self.sort_blocks_newest_first();
        if matches!(
            delta,
            Some(GroupDelta::BlockInserted(_) | GroupDelta::BlockReplaced(_))
        ) {
            delta = delta.and_then(|d| self.reindex_delta(d, &event.id));
        }
        delta
    }

    /// Process a removed event. Returns at most one delta.
    #[must_use]
    pub fn on_remove(&mut self, id: &EventId) -> Option<GroupDelta> {
        self.by_id.remove(id);
        self.pending_ancestor_ids.remove(id);
        self.orphaned.remove(id);
        self.orphans.retain(|_, set| {
            set.remove(id);
            !set.is_empty()
        });

        // Supersession bookkeeping.
        //   - if `id` was itself a target, drop its row (the surviving
        //     superseders no longer have a target to preempt)
        //   - if `id` was a superseder, scrub it from every set; collect
        //     targets whose set is now empty so we can restore their blocks
        self.superseded_by.remove(id);
        let mut restore_candidates: Vec<EventId> = Vec::new();
        self.superseded_by.retain(|target, set| {
            set.remove(id);
            if set.is_empty() {
                restore_candidates.push(target.clone());
                false
            } else {
                true
            }
        });

        let block_delta = self.remove_id_from_blocks(id);

        // Restore unsuperseded targets that still have payloads on hand.
        let mut restored_any = false;
        for target in restore_candidates {
            if self.seen.contains(&target) {
                continue;
            }
            let Some(event) = self.by_id.get(&target).cloned() else {
                continue;
            };
            let _ = self.place_event(&event);
            restored_any = true;
        }

        if block_delta.is_some() || restored_any {
            self.collapse_adjacent();
            self.sort_blocks_newest_first();
        }

        block_delta
    }

    fn remove_id_from_blocks(&mut self, id: &EventId) -> Option<GroupDelta> {
        if !self.seen.remove(id) {
            return None;
        }

        let mut removed_idx: Option<usize> = None;
        let mut block_replaced_idx: Option<usize> = None;

        for (idx, block) in self.blocks.iter_mut().enumerate() {
            match block {
                TimelineBlock::Standalone(eid) if eid == id => {
                    removed_idx = Some(idx);
                    break;
                }
                TimelineBlock::Module {
                    events, has_gap, ..
                } => {
                    if events.iter().any(|e| e == id) {
                        events.retain(|e| e != id);
                        // A removed mid-chain element introduces a gap.
                        *has_gap = true;
                        if events.is_empty() {
                            removed_idx = Some(idx);
                        } else if events.len() == 1 {
                            let only = events.remove(0);
                            *block = TimelineBlock::Standalone(only);
                            block_replaced_idx = Some(idx);
                        } else {
                            block_replaced_idx = Some(idx);
                        }
                        break;
                    }
                }
                TimelineBlock::Standalone(_) => {}
            }
        }

        if let Some(idx) = removed_idx {
            self.blocks.remove(idx);
            Some(GroupDelta::BlockRemoved(idx))
        } else {
            block_replaced_idx.map(GroupDelta::BlockReplaced)
        }
    }

    /// Evict `id` from the layout when it appears as a `Standalone` block.
    /// `Module` membership is left intact — a reposted note that's also
    /// anchoring a reply chain stays in the chain so the reply still has
    /// parent context. The event's payload remains in `by_id` so the block
    /// can be restored if every superseder is later removed.
    fn unplace_standalone(&mut self, id: &EventId) {
        let standalone_idx = self.blocks.iter().position(|block| match block {
            TimelineBlock::Standalone(eid) => eid == id,
            TimelineBlock::Module { .. } => false,
        });
        if let Some(idx) = standalone_idx {
            self.blocks.remove(idx);
            self.seen.remove(id);
        }
    }

    /// Process a replaced event. Modelled as remove + insert; wrappers see a
    /// single delta — the inserted one.
    #[must_use]
    pub fn on_replace(&mut self, old_id: &EventId, new_event: &KernelEvent) -> Option<GroupDelta> {
        let _ = self.on_remove(old_id); // delta from remove is subsumed by the insert delta below
        self.on_insert(new_event)
    }

    // ── Placement helpers ───────────────────────────────────────────────

    fn place_event(&mut self, event: &KernelEvent) -> Option<GroupDelta> {
        if self.seen.contains(&event.id) {
            return None;
        }

        let parent = self.resolver.parent(event);
        let root_hint = self.resolver.root(event);

        // Case A: parent is an Event in store → try to splice onto the block
        // whose leaf is that parent (promoting Standalone → Module as
        // needed). If extension would exceed `max_module_size`, fall through
        // to Case B to spawn a new block.
        if let Some(ThreadPointer::Event { id: parent_id, .. }) = &parent {
            if !self.by_id.contains_key(parent_id) || self.orphaned.contains(parent_id) {
                // Parent isn't placed yet (unknown locally, or buffered
                // awaiting its own parent). Buffer this child too — it
                // stitches in when the chain settles top-down.
                self.orphans
                    .entry(parent_id.clone())
                    .or_default()
                    .insert(event.id.clone());
                self.orphaned.insert(event.id.clone());
                self.pending_ancestor_ids.insert(parent_id.clone());
                return None;
            }

            if let Some(idx) = self.find_block_with_leaf(parent_id) {
                let parent_kev = self.by_id.get(parent_id).cloned();
                let extended =
                    self.try_extend_block(idx, event, parent_kev.as_ref(), root_hint.as_ref());
                if extended {
                    self.seen.insert(event.id.clone());
                    self.orphaned.remove(&event.id);
                    self.pending_ancestor_ids.remove(&event.id);
                    return Some(GroupDelta::BlockReplaced(idx));
                }
            }
        }

        // Case B: build a fresh chain by walking ancestors.
        let (chain, terminal_root, has_gap) = self.walk_chain(event, parent.as_ref(), root_hint);
        for id in &chain {
            self.seen.insert(id.clone());
            self.orphaned.remove(id);
            self.pending_ancestor_ids.remove(id);
        }

        // `walk_chain` always seeds the chain with `event.id`, so it is
        // non-empty in practice. If that invariant is ever violated we
        // degrade silently (skip placement) rather than panic across the
        // public API boundary.
        let block = match chain.as_slice() {
            [_] => TimelineBlock::Standalone(chain.into_iter().next()?),
            [] => return None,
            _ => TimelineBlock::Module {
                events: chain,
                has_gap,
                root: terminal_root,
            },
        };
        self.blocks.insert(0, block);
        Some(GroupDelta::BlockInserted(0))
    }

    /// Try to splice `event` onto the block at `idx` whose leaf is its
    /// parent. Returns true on success (block in-place mutated); false when
    /// `max_module_size` is exceeded (caller falls back to a fresh block).
    fn try_extend_block(
        &mut self,
        idx: usize,
        event: &KernelEvent,
        parent_kev: Option<&KernelEvent>,
        root_hint: Option<&ThreadPointer>,
    ) -> bool {
        let max_size = self.policy.max_module_size as usize;
        let gap_threshold = self.policy.max_lookback_gap_secs;
        let leaf_gap = gap_between(parent_kev, Some(event), gap_threshold);

        match &mut self.blocks[idx] {
            TimelineBlock::Standalone(parent_id) => {
                if max_size < 2 {
                    return false;
                }
                let mismatched = root_id_mismatched(root_hint, parent_id.as_str());
                let promoted = TimelineBlock::Module {
                    events: vec![parent_id.clone(), event.id.clone()],
                    has_gap: leaf_gap || mismatched,
                    root: root_hint.cloned(),
                };
                self.blocks[idx] = promoted;
                true
            }
            TimelineBlock::Module {
                events,
                has_gap,
                root,
            } => {
                if events.len() >= max_size {
                    return false;
                }
                events.push(event.id.clone());
                *has_gap = *has_gap || leaf_gap;
                if root.is_none() {
                    *root = root_hint.cloned();
                }
                // Mismatched root: chain top is not the declared root id.
                // `events` was just pushed to above, so `first()` is `Some`
                // in practice; the `if let` keeps a panic off the public
                // API path if that ever stops holding.
                if let Some(top) = events.first() {
                    if root_id_mismatched(root.as_ref(), top) {
                        *has_gap = true;
                    }
                }
                true
            }
        }
    }

    /// Find a block whose leaf (last event) equals `parent_id`. Walks both
    /// Standalone and Module blocks.
    fn find_block_with_leaf(&self, parent_id: &str) -> Option<usize> {
        self.blocks.iter().position(|b| match b {
            TimelineBlock::Standalone(id) => id == parent_id,
            TimelineBlock::Module { events, .. } => {
                events.last().is_some_and(|leaf| leaf == parent_id)
            }
        })
    }

    /// Walk up to `max_ancestor_hops` from `event`. Returns the chain in
    /// root-first order (oldest first), the terminal root pointer (if non-
    /// Event), and whether a gap was detected.
    fn walk_chain(
        &mut self,
        event: &KernelEvent,
        initial_parent: Option<&ThreadPointer>,
        root_hint: Option<ThreadPointer>,
    ) -> (Vec<EventId>, Option<ThreadPointer>, bool) {
        let mut chain: Vec<EventId> = vec![event.id.clone()];
        let mut has_gap = false;
        let mut terminal_root: Option<ThreadPointer> = None;
        let max_size = self.policy.max_module_size as usize;
        let max_hops = self.policy.max_ancestor_hops as usize;

        let mut cursor: Option<ThreadPointer> = initial_parent.cloned();
        let mut hops_used = 0usize;

        while let Some(ptr) = cursor.take() {
            if hops_used >= max_hops {
                if !matches!(ptr, ThreadPointer::Event { .. }) {
                    terminal_root = Some(ptr.clone());
                } else if let ThreadPointer::Event { id, .. } = &ptr {
                    if !self.by_id.contains_key(id) {
                        has_gap = true;
                        self.pending_ancestor_ids.insert(id.clone());
                    }
                }
                break;
            }

            match ptr {
                ThreadPointer::Event { id, .. } => {
                    if self.seen.contains(&id) || self.orphaned.contains(&id) {
                        // Parent already lives in another block, or it's
                        // itself buffered awaiting its own parent. Either
                        // way we do not steal it — adjacent-root collapse
                        // or top-down orphan replay will reconcile.
                        has_gap = true;
                        break;
                    }
                    let Some(parent_event) = self.by_id.get(&id).cloned() else {
                        has_gap = true;
                        self.pending_ancestor_ids.insert(id.clone());
                        break;
                    };
                    // `chain` is seeded non-empty and only ever grows, so
                    // `first()` is `Some` in practice. The `if let` keeps a
                    // panic off the public API path; the gap check is purely
                    // additive, so skipping it on an empty chain is safe.
                    if let Some(child_id) = chain.first() {
                        let child = self.by_id.get(child_id);
                        if gap_between(
                            Some(&parent_event),
                            child,
                            self.policy.max_lookback_gap_secs,
                        ) {
                            has_gap = true;
                        }
                    }
                    chain.insert(0, id.clone());
                    if chain.len() >= max_size {
                        break;
                    }
                    cursor = self.resolver.parent(&parent_event);
                    hops_used += 1;
                }
                other => {
                    terminal_root = Some(other);
                    break;
                }
            }
        }

        // Mismatched-root detection: chain top is not the declared root id.
        // `chain` is non-empty in practice; the `if let` keeps a panic off
        // the public API path, and this diagnostic is purely additive.
        if let Some(ThreadPointer::Event { id: rid, .. }) =
            terminal_root.as_ref().or(root_hint.as_ref())
        {
            if let Some(top) = chain.first() {
                if top != rid {
                    has_gap = true;
                }
            }
        }

        // Adopt root_hint when nothing terminal was hit (used purely for
        // adjacent-block collapse).
        if terminal_root.is_none() {
            terminal_root = root_hint;
        }

        (chain, terminal_root, has_gap)
    }

    fn sort_blocks_newest_first(&mut self) {
        let by_id = &self.by_id;
        self.blocks
            .sort_by(|a, b| block_sort_key(b, by_id).cmp(&block_sort_key(a, by_id)));
    }

    fn reindex_delta(&self, delta: GroupDelta, event_id: &str) -> Option<GroupDelta> {
        let idx = self.find_block_containing(event_id)?;
        Some(match delta {
            GroupDelta::BlockInserted(_) => GroupDelta::BlockInserted(idx),
            GroupDelta::BlockReplaced(_) => GroupDelta::BlockReplaced(idx),
            GroupDelta::BlockRemoved(idx) => GroupDelta::BlockRemoved(idx),
        })
    }

    fn find_block_containing(&self, event_id: &str) -> Option<usize> {
        self.blocks.iter().position(|block| match block {
            TimelineBlock::Standalone(id) => id == event_id,
            TimelineBlock::Module { events, .. } => events.iter().any(|id| id == event_id),
        })
    }
}

fn block_sort_key(block: &TimelineBlock, by_id: &BTreeMap<EventId, KernelEvent>) -> (u64, EventId) {
    match block {
        TimelineBlock::Standalone(id) => by_id
            .get(id)
            .map(|event| (event.created_at, event.id.clone()))
            .unwrap_or_else(|| (0, id.clone())),
        TimelineBlock::Module { events, .. } => events
            .iter()
            .filter_map(|id| {
                by_id
                    .get(id)
                    .map(|event| (event.created_at, event.id.clone()))
            })
            .max()
            .unwrap_or_else(|| (0, events.first().cloned().unwrap_or_default())),
    }
}
