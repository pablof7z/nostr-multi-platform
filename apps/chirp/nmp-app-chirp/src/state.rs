//! `ChirpModularTimeline` — the per-app projection.
//!
//! Owns one `Nip10ModularTimelineView` state plus the lookup metadata Swift
//! needs to render the blocks (per-event author / content / timestamp). The
//! grouper itself works on `EventId`s only; the lookup table lives here so
//! the FFI snapshot is self-describing.

use std::collections::HashMap;
use std::sync::Mutex;

use nmp_core::substrate::{KernelEvent, ViewContext, ViewModule};
use nmp_core::KernelEventObserver;
use nmp_nip01::{
    ModularTimelinePayload, ModularTimelineSpec, ModularTimelineState, Nip10ModularTimelineView,
};

use crate::payload::{ChirpEventCard, ChirpTimelineSnapshot};

/// Owned state for the Chirp modular timeline projection.
///
/// `Mutex` because the `KernelEventObserver::on_kernel_event` method takes
/// `&self` (it's called from the actor thread; the FFI snapshot side may run
/// on a Swift caller thread). Contention is low — the actor fires one event
/// at a time, the FFI snapshot is rare relative to ingest.
pub struct ChirpModularTimeline {
    inner: Mutex<Inner>,
}

struct Inner {
    state: ModularTimelineState,
    /// Per-event cards for FFI snapshots. `TimelineBlock` only carries
    /// `EventId`s; the renderer needs author/content/timestamp too. Cards
    /// land here on every admitted event. Bounded by the projection's own
    /// retention — the grouper currently holds every accepted event for
    /// chain stitching (no eviction yet). M2 follow-up: prune cards for ids
    /// no longer reachable from any block.
    cards: HashMap<String, ChirpEventCard>,
}

impl ChirpModularTimeline {
    /// Open the view for the given spec. The spec carries the viewer pubkey
    /// (for future personalization keys), the kinds to admit (defaults to
    /// `[1]`), the optional author filter, and the grouping policy
    /// (default: 3-event modules, 72h gap threshold, 2 ancestor hops).
    pub fn new(spec: ModularTimelineSpec) -> Self {
        let ctx = ViewContext::default();
        let (state, _payload) = Nip10ModularTimelineView::open(&ctx, spec);
        Self {
            inner: Mutex::new(Inner {
                state,
                cards: HashMap::new(),
            }),
        }
    }

    /// Snapshot of the current blocks + the per-event cards Swift renders.
    /// Empty if the grouper has accepted no events yet. The snapshot owns
    /// its data — the lock is released before serialization.
    pub fn snapshot(&self) -> ChirpTimelineSnapshot {
        let Ok(inner) = self.inner.lock() else {
            // D6 — poisoned mutex on the projection side returns an empty
            // snapshot rather than panicking. The next successful event
            // ingest will heal the projection's payload.
            return ChirpTimelineSnapshot::empty();
        };
        let ctx = ViewContext::default();
        let payload: ModularTimelinePayload =
            Nip10ModularTimelineView::snapshot(&ctx, &inner.state);
        ChirpTimelineSnapshot {
            blocks: payload.blocks,
            cards: inner.cards.values().cloned().collect(),
        }
    }
}

impl KernelEventObserver for ChirpModularTimeline {
    fn on_kernel_event(&self, event: &KernelEvent) {
        let Ok(mut inner) = self.inner.lock() else {
            // D6 — poisoned mutex silently no-ops; the next successful call
            // (after Rust unpoisons) resumes ingest.
            return;
        };
        let ctx = ViewContext::default();
        // The view's own `admits()` check filters by kind/author (the spec
        // carries the gating). We always cache the card before calling the
        // view so even a rejected event has its display metadata ready if a
        // later admit references it — kind:1 reposts (kind:6) skip the
        // module but should still be addressable.
        inner.cards.insert(event.id.clone(), ChirpEventCard::from(event));
        Nip10ModularTimelineView::on_event_inserted(&ctx, &mut inner.state, event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_threading::{ModulePolicy, TimelineBlock};
    use std::sync::Arc;

    fn spec() -> ModularTimelineSpec {
        ModularTimelineSpec {
            viewer: "me".into(),
            kinds: vec![],
            authors: None,
            policy: ModulePolicy::default(),
        }
    }

    fn note(id: &str, ts: u64, tags: Vec<Vec<String>>) -> KernelEvent {
        KernelEvent {
            id: id.into(),
            author: "auth".into(),
            kind: 1,
            created_at: ts,
            tags,
            content: id.into(),
        }
    }

    fn reply_to(id: &str, ts: u64, root: &str, parent: &str) -> KernelEvent {
        note(
            id,
            ts,
            vec![
                vec!["e".into(), root.into(), "".into(), "root".into()],
                vec!["e".into(), parent.into(), "".into(), "reply".into()],
            ],
        )
    }

    #[test]
    fn empty_open_yields_empty_snapshot() {
        let proj = ChirpModularTimeline::new(spec());
        let snap = proj.snapshot();
        assert!(snap.blocks.is_empty());
        assert!(snap.cards.is_empty());
    }

    #[test]
    fn root_plus_reply_collapses_into_one_module() {
        let proj = ChirpModularTimeline::new(spec());
        proj.on_kernel_event(&note("R", 1, vec![]));
        proj.on_kernel_event(&reply_to("C", 2, "R", "R"));
        let snap = proj.snapshot();
        assert_eq!(snap.blocks.len(), 1);
        match &snap.blocks[0] {
            TimelineBlock::Module { events, .. } => {
                assert_eq!(events, &vec!["R".to_string(), "C".to_string()]);
            }
            other => panic!("expected Module, got {other:?}"),
        }
        assert_eq!(snap.cards.len(), 2);
    }

    #[test]
    fn standalone_event_becomes_standalone_block() {
        let proj = ChirpModularTimeline::new(spec());
        proj.on_kernel_event(&note("S", 1, vec![]));
        let snap = proj.snapshot();
        assert_eq!(snap.blocks.len(), 1);
        assert!(matches!(snap.blocks[0], TimelineBlock::Standalone(_)));
    }

    #[test]
    fn observer_trait_object_drives_grouper() {
        // Exercise the typed Rust observer registration path through an
        // `Arc<dyn KernelEventObserver>`. `nmp-app-chirp::ChirpModularTimeline`
        // is intended to be plugged into `NmpApp::register_event_observer`
        // exactly this way.
        let proj: Arc<dyn KernelEventObserver> = Arc::new(ChirpModularTimeline::new(spec()));
        proj.on_kernel_event(&note("X", 1, vec![]));
    }
}
