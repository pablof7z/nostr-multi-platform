//! Owns block recency ordering — keeping `blocks` sorted newest-first after
//! every mutation — and delta-index resolution when a mutation moves a
//! block's position in the display list.

use std::collections::BTreeMap;

use nmp_core::substrate::{EventId, KernelEvent};

use crate::block::TimelineBlock;
use crate::resolver::ParentResolver;

use super::{GroupDelta, Grouper};

impl<R: ParentResolver> Grouper<R> {
    pub(super) fn sort_blocks_newest_first(&mut self) {
        let by_id = &self.by_id;
        self.blocks
            .sort_by_key(|b| std::cmp::Reverse(block_sort_key(b, by_id)));
    }

    pub(super) fn reindex_delta(&self, delta: GroupDelta, event_id: &str) -> Option<GroupDelta> {
        let idx = self.find_block_containing(event_id)?;
        Some(match delta {
            GroupDelta::BlockInserted(_) => GroupDelta::BlockInserted(idx),
            GroupDelta::BlockReplaced(_) => GroupDelta::BlockReplaced(idx),
            GroupDelta::BlockRemoved(idx) => GroupDelta::BlockRemoved(idx),
        })
    }

    fn find_block_containing(&self, event_id: &str) -> Option<usize> {
        self.blocks.iter().position(|block| match block {
            TimelineBlock::Standalone { id, .. } => id == event_id,
            TimelineBlock::Module { events, .. } => events.iter().any(|id| id == event_id),
        })
    }
}

fn block_sort_key(block: &TimelineBlock, by_id: &BTreeMap<EventId, KernelEvent>) -> (u64, EventId) {
    match block {
        TimelineBlock::Standalone { id, .. } => by_id
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
