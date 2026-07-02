//! Controller-level tests for the single pull paging path (ADR-0058 §8 step-6B).
//!
//! These exercise `PullFeedController::load_older` end-to-end with stub closures
//! (no real `Kernel`): a fake `pull_fn` feeds canned pages, a fake `apply`
//! records what the feed ingested, and a fake `advance` counts viewport grows.
//! Together they prove the 6B contract: late-old completeness rides seq, display
//! order is left to the snapshot, empty-advancing drains do not loop, `PullGap`
//! rebases explicitly, inexpressible shapes fail closed, and nothing consumes a
//! wake.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::KernelEvent;
use nmp_core::PullScope;
use nmp_planner::InterestShape;
use nmp_store::{LogOp, PullPage, RawEvent, ScanLogResult, StoreLogEntry};

use super::{
    ClosureInterestShape, FeedAdvance, FeedApply, FeedReplace, FeedReset, PullFeedController,
    PullFn,
};

mod basic_pull;
mod reset_replace;

// ─── fixtures ────────────────────────────────────────────────────────────────

pub(super) fn real_shape() -> Arc<dyn crate::FeedInterestShape + Send + Sync> {
    Arc::new(ClosureInterestShape::new(|| {
        Some(InterestShape {
            authors: ["a".repeat(64)].into_iter().collect(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        })
    }))
}

pub(super) fn opaque_shape() -> Arc<dyn crate::FeedInterestShape + Send + Sync> {
    Arc::new(ClosureInterestShape::new(|| None))
}

pub(super) fn raw(id: &str, created_at: u64) -> RawEvent {
    RawEvent {
        id: id.to_string(),
        pubkey: "a".repeat(64),
        created_at,
        kind: 1,
        tags: vec![],
        content: String::new(),
        sig: "0".repeat(128),
    }
}

pub(super) fn inserted(seq: u64, id: &str, created_at: u64) -> StoreLogEntry {
    StoreLogEntry {
        seq,
        op: LogOp::Inserted,
        event_id: [seq as u8; 32],
        raw_event: Some(raw(id, created_at)),
        source_relay: Some("wss://r/".to_string()),
        received_at_ms: 0,
    }
}

/// A `Replaced` log row: a replaceable event (`id`/`created_at`) superseding an
/// earlier version whose 32-byte id is all-`replaced_byte`. The superseded id's
/// hex form is what the controller hands the `FeedReplace` hook.
pub(super) fn replaced(seq: u64, id: &str, created_at: u64, replaced_byte: u8) -> StoreLogEntry {
    StoreLogEntry {
        seq,
        op: LogOp::Replaced {
            replaced_id: [replaced_byte; 32],
        },
        event_id: [seq as u8; 32],
        raw_event: Some(raw(id, created_at)),
        source_relay: Some("wss://r/".to_string()),
        received_at_ms: 0,
    }
}

/// The lowercase-hex id the controller derives from an all-`byte` 32-byte id.
pub(super) fn hex32(byte: u8) -> String {
    (0..32).map(|_| format!("{byte:02x}")).collect()
}

pub(super) fn page(
    entries: Vec<StoreLogEntry>,
    next_after_seq: u64,
    latest_seq: u64,
) -> ScanLogResult {
    ScanLogResult::Page(PullPage {
        entries,
        next_after_seq,
        latest_seq,
        has_more: next_after_seq < latest_seq,
    })
}

/// A test feed: an in-memory ingest sink (deduped by id) plus a viewport-grow
/// counter, mirroring the engine's `apply` + `grow_visible_window` seam.
#[derive(Default)]
pub(super) struct FakeFeed {
    pub ingested: Mutex<Vec<KernelEvent>>,
    pub grows: AtomicUsize,
    pub resets: AtomicUsize,
    pub removed: Mutex<Vec<String>>,
}

impl FakeFeed {
    pub(super) fn apply(self: &Arc<Self>) -> FeedApply {
        let me = Arc::clone(self);
        Arc::new(move |ev: &KernelEvent| {
            let mut g = me.ingested.lock().unwrap();
            if g.iter().any(|e| e.id == ev.id) {
                false
            } else {
                g.push(ev.clone());
                true
            }
        })
    }
    pub(super) fn advance(self: &Arc<Self>) -> FeedAdvance {
        let me = Arc::clone(self);
        Arc::new(move || {
            me.grows.fetch_add(1, Ordering::Relaxed);
        })
    }
    /// A reset hook that drops all ingested rows, counts each call, and reports
    /// whether anything was actually cleared — mirroring
    /// `FlatFeed::reset_for_perspective_change`.
    pub(super) fn reset(self: &Arc<Self>) -> FeedReset {
        let me = Arc::clone(self);
        Arc::new(move || {
            me.resets.fetch_add(1, Ordering::Relaxed);
            let mut g = me.ingested.lock().unwrap();
            let had_rows = !g.is_empty();
            g.clear();
            had_rows
        })
    }
    /// A replace hook that evicts a source by id and records the eviction.
    pub(super) fn replace(self: &Arc<Self>) -> FeedReplace {
        let me = Arc::clone(self);
        Arc::new(move |source_id: &str| {
            me.removed.lock().unwrap().push(source_id.to_string());
            me.ingested.lock().unwrap().retain(|e| e.id != source_id);
        })
    }
    /// Display order the snapshot would render: newest-first by `(created_at, id)`.
    pub(super) fn display_order(&self) -> Vec<String> {
        let g = self.ingested.lock().unwrap();
        let mut keyed: Vec<(u64, String)> =
            g.iter().map(|e| (e.created_at, e.id.clone())).collect();
        keyed.sort_by(|(lt, lid), (rt, rid)| rt.cmp(lt).then_with(|| rid.cmp(lid)));
        keyed.into_iter().map(|(_, id)| id).collect()
    }
}

/// Build a controller whose `pull_fn` replays `pages` (one per call) and records
/// every `after_seq` it was asked for.
pub(super) fn controller_with_pages(
    feed: &Arc<FakeFeed>,
    pages: Vec<ScanLogResult>,
    seen_after: Arc<Mutex<Vec<u64>>>,
) -> Arc<PullFeedController> {
    let queue = Arc::new(Mutex::new(std::collections::VecDeque::from(pages)));
    let pull: PullFn = {
        let queue = Arc::clone(&queue);
        let seen = Arc::clone(&seen_after);
        Arc::new(move |_scope: PullScope, after_seq: u64, _limits| {
            seen.lock().unwrap().push(after_seq);
            queue
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| page(vec![], after_seq, after_seq))
        })
    };
    PullFeedController::new(real_shape(), pull, feed.apply(), feed.advance())
}
