//! Owns event placement: splicing a new event onto the block whose leaf is
//! its parent (promoting `Standalone` → `Module` as needed), or — when
//! splicing isn't possible — walking ancestors to build a fresh chain up to
//! the policy's hop and size limits.

use nmp_core::substrate::{EventId, KernelEvent};

use crate::block::TimelineBlock;
use crate::pointer::ThreadPointer;
use crate::resolver::ParentResolver;

use super::collapse::{gap_between, root_id_mismatched};
use super::{GroupDelta, Grouper};

impl<R: ParentResolver> Grouper<R> {
    pub(super) fn place_event(&mut self, event: &KernelEvent) -> Option<GroupDelta> {
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
            // A length-1 chain is still a reply when `terminal_root` is
            // set (parent absent / leaf taken / max_module_size hit). Carry
            // the root so the block is not mistaken for a thread root.
            [_] => TimelineBlock::Standalone {
                id: chain.into_iter().next()?,
                root: terminal_root,
            },
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
            TimelineBlock::Standalone { id: parent_id, .. } => {
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
            TimelineBlock::Standalone { id, .. } => id == parent_id,
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
}
