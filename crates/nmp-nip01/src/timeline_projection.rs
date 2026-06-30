//! NIP-10 modular timeline projection.
//!
//! This projection owns protocol grouping state only: event ids arranged into
//! NIP-10 timeline blocks. Concrete feed rows, render payloads, repost
//! composition, and typed feed wire live in higher composition crates.

use std::sync::{Arc, Mutex};

use nmp_core::substrate::{
    empty_suppression_lookup, BoundedMessageMap, KernelEvent, SuppressionLookup, ViewContext,
    MAX_PROJECTION_MESSAGES,
};
use nmp_core::ObservedProjectionSink;
use nmp_threading::TimelineBlock;
use serde::{Deserialize, Serialize};

use crate::meta_timeline::{
    ModularTimelinePayload, ModularTimelineSpec, ModularTimelineState, Nip10ModularTimelineView,
};
use crate::profile_display::profile_from_event;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ModularTimelineSnapshot {
    pub blocks: Vec<TimelineBlock>,
}

impl ModularTimelineSnapshot {
    #[must_use]
    pub fn empty() -> Self {
        Self { blocks: Vec::new() }
    }
}

pub struct ModularTimelineProjection {
    inner: Mutex<Inner>,
    /// Substrate-generic suppression lookup injected by the composition owner.
    suppression: Arc<dyn SuppressionLookup>,
}

struct Inner {
    state: ModularTimelineState,
    events: BoundedMessageMap<String, TimelineEventIndex>,
}

#[derive(Clone, Debug)]
struct TimelineEventIndex {
    author_pubkey: String,
    created_at: u64,
}

impl ModularTimelineProjection {
    #[must_use]
    pub fn new(spec: &ModularTimelineSpec) -> Self {
        let ctx = ViewContext::default();
        let (state, _payload) = Nip10ModularTimelineView::open(&ctx, spec);
        Self {
            inner: Mutex::new(Inner {
                state,
                events: BoundedMessageMap::new(MAX_PROJECTION_MESSAGES),
            }),
            suppression: empty_suppression_lookup(),
        }
    }

    /// Wire a suppression lookup, for example `nmp-nip51`'s mute projection.
    pub fn set_suppression(&mut self, lookup: Arc<dyn SuppressionLookup>) {
        self.suppression = lookup;
    }

    #[must_use]
    pub fn snapshot(&self) -> ModularTimelineSnapshot {
        let Ok(inner) = self.inner.lock() else {
            return ModularTimelineSnapshot::empty();
        };
        let blocks = sorted_projection_blocks(&inner);
        let blocks = suppress_blocks(&blocks, &inner.events, &*self.suppression);
        ModularTimelineSnapshot { blocks }
    }
}

impl ObservedProjectionSink for ModularTimelineProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if self.suppression.is_suppressed_author(&event.author)
            || self.suppression.is_suppressed_event(&event.id)
        {
            return;
        }

        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        if profile_from_event(event).is_some() {
            return;
        }
        if crate::try_from_kernel_event(event).is_some() {
            inner.events.insert(
                event.id.clone(),
                TimelineEventIndex {
                    author_pubkey: event.author.clone(),
                    created_at: event.created_at,
                },
            );
        }
        let ctx = ViewContext::default();
        let _ = Nip10ModularTimelineView::on_event_inserted(&ctx, &mut inner.state, event);
    }
}

fn sorted_projection_blocks(inner: &Inner) -> Vec<TimelineBlock> {
    let ctx = ViewContext::default();
    let payload: ModularTimelinePayload = Nip10ModularTimelineView::snapshot(&ctx, &inner.state);
    let mut blocks = payload.blocks;
    blocks.sort_by(|left, right| {
        let left_cursor = block_sort_cursor(left, &inner.events);
        let right_cursor = block_sort_cursor(right, &inner.events);
        right_cursor.cmp(&left_cursor)
    });
    blocks
}

fn block_sort_cursor(
    block: &TimelineBlock,
    events: &BoundedMessageMap<String, TimelineEventIndex>,
) -> (u64, String) {
    block_event_ids(block)
        .into_iter()
        .filter_map(|id| events.get(&id).map(|event| (event.created_at, id)))
        .max()
        .unwrap_or_default()
}

fn suppress_blocks(
    blocks: &[TimelineBlock],
    events: &BoundedMessageMap<String, TimelineEventIndex>,
    suppression: &dyn SuppressionLookup,
) -> Vec<TimelineBlock> {
    blocks
        .iter()
        .filter(|block| {
            let Some(root_id) = block_event_ids(block).into_iter().next() else {
                return true;
            };
            if suppression.is_suppressed_event(&root_id) {
                return false;
            }
            events
                .get(&root_id)
                .map(|event| !suppression.is_suppressed_author(&event.author_pubkey))
                .unwrap_or(true)
        })
        .cloned()
        .collect()
}

fn block_event_ids(block: &TimelineBlock) -> Vec<String> {
    match block {
        TimelineBlock::Standalone { id, .. } => vec![id.clone()],
        TimelineBlock::Module { events, .. } => events.clone(),
    }
}

#[cfg(test)]
#[path = "timeline_projection/tests.rs"]
mod tests;
