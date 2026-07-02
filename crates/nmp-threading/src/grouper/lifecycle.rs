//! Owns the grouper's mutation entry points — insert, remove, and replace —
//! along with the orphan-replay and supersession bookkeeping that keeps
//! those transitions consistent. Placement of an accepted event into the
//! block layout itself lives in [`super::placement`].

use nmp_core::substrate::{EventId, KernelEvent};

use crate::block::TimelineBlock;
use crate::resolver::ParentResolver;

use super::{GroupDelta, Grouper};

impl<R: ParentResolver> Grouper<R> {
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
                TimelineBlock::Standalone { id: eid, .. } if eid == id => {
                    removed_idx = Some(idx);
                    break;
                }
                TimelineBlock::Module {
                    events,
                    has_gap,
                    root,
                } => {
                    if events.iter().any(|e| e == id) {
                        events.retain(|e| e != id);
                        // A removed mid-chain element introduces a gap.
                        *has_gap = true;
                        if events.is_empty() {
                            removed_idx = Some(idx);
                        } else if events.len() == 1 {
                            let only = events.remove(0);
                            // Collapsing a module to a single event must keep
                            // the module's resolved root pointer — otherwise
                            // the survivor reads as a thread root rather than
                            // the partial-chain head it actually is.
                            *block = TimelineBlock::Standalone {
                                id: only,
                                root: root.take(),
                            };
                            block_replaced_idx = Some(idx);
                        } else {
                            block_replaced_idx = Some(idx);
                        }
                        break;
                    }
                }
                TimelineBlock::Standalone { .. } => {}
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
            TimelineBlock::Standalone { id: eid, .. } => eid == id,
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
}
