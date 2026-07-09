//! #3088 — regression coverage for the live-activation shape source used by
//! `Kernel::open_interest_with_observer_replay`.
//!
//! Split into a sibling module (mirrors `observer_replay_store_tests`'s
//! self-contained helper duplication) to keep `observer_replay_tests` within
//! the 500 LOC ceiling (AGENTS.md § file-size rules).

use super::*;
use crate::actor::{new_event_observer_slot, register_rust_observer_muted, ObservedProjectionSink};
use crate::kernel::observer_replay::ObserverReplayRequest;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::{InsertOutcome, RawEvent, VerifiedEvent};
use crate::substrate::KernelEvent;
use std::sync::{Arc, Mutex};

struct CapturingObserver {
    events: Mutex<Vec<KernelEvent>>,
}

impl CapturingObserver {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            events: Mutex::new(Vec::new()),
        })
    }

    fn count(&self) -> usize {
        self.events.lock().unwrap().len()
    }
}

impl ObservedProjectionSink for CapturingObserver {
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.events.lock().unwrap().push(event.clone());
    }
}

/// Ingest a minimal kind:1 event into the kernel and fire the global fan-out
/// (mirrors `observer_replay_tests::ingest`).
fn ingest(kernel: &mut Kernel, id: &str, author: &str, created_at: u64) {
    let raw = RawEvent {
        id: id.to_string(),
        pubkey: author.to_string(),
        created_at,
        kind: 1,
        tags: vec![],
        content: "test".into(),
        sig: "a".repeat(128),
    };
    let proceed = kernel
        .store
        .insert(
            VerifiedEvent::from_raw_unchecked(raw.clone()),
            &"test-relay".to_string(),
            created_at,
        )
        .map(|outcome| {
            matches!(
                outcome,
                InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. }
            )
        })
        .unwrap_or(false);
    if !proceed {
        return;
    }
    let cached = StoredEvent {
        id: raw.id.clone(),
        author: raw.pubkey.clone(),
        kind: raw.kind,
        created_at: raw.created_at,
        tags: raw.tags.clone(),
        content: raw.content.clone(),
        relay_count: 1,
    };
    kernel.events.insert(raw.id.clone(), cached.clone());
    kernel.notify_event_observers(&KernelEvent {
        id: cached.id,
        author: cached.author,
        kind: cached.kind,
        created_at: cached.created_at,
        tags: cached.tags,
        content: cached.content,
        relay_provenance: Vec::new(),
    });
}

fn author_filter_json(author: &str) -> String {
    format!(r#"{{"kinds":[1],"authors":["{author}"]}}"#)
}

fn sub_identity(filter_json: &str, consumer_id: &str, scope: u32) -> crate::subs::SubIdentity {
    crate::subs::interest_builder::build_interest_pair(
        filter_json,
        consumer_id,
        scope,
        None,
        false,
        crate::planner::InterestLifecycle::Tailing,
    )
    .map(|(id, _)| id)
    .expect("valid filter -> identity")
}

fn logical_interest(
    filter_json: &str,
    consumer_id: &str,
    scope: u32,
) -> crate::planner::LogicalInterest {
    crate::subs::interest_builder::build_interest_pair(
        filter_json,
        consumer_id,
        scope,
        None,
        false,
        crate::planner::InterestLifecycle::Tailing,
    )
    .map(|(_, interest)| interest)
    .expect("valid filter -> interest")
}

/// #3088 fix follow-up: `ObservedProjectionCommandHandle::open_live_only`
/// (NIP-50 search) deliberately clears `replay_shapes` to empty so stale
/// structural cache replay cannot bypass its own query filter. Activation
/// must still fall back to the interest's own shape in that case, or a
/// live-only observer would never receive anything at all.
#[test]
fn empty_replay_shapes_falls_back_to_interest_shape_for_activation() {
    let slot = new_event_observer_slot();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_event_observers_handle(slot.clone());

    let author = "9".repeat(64);
    let capturing = CapturingObserver::new();
    let observer_id = register_rust_observer_muted(&slot, capturing.clone());

    let filter_json = author_filter_json(&author);
    let identity = sub_identity(&filter_json, "test-consumer-live-only", 1);
    let interest = logical_interest(&filter_json, "test-consumer-live-only", 1);
    // Mirrors `open_with_replay`'s `!replay` branch: empty shapes, zero limit.
    let replay = ObserverReplayRequest {
        observer_id,
        shapes: vec![],
        limit: 0,
    };
    kernel.open_interest_with_observer_replay(identity, interest, replay, "test");

    assert_eq!(
        capturing.count(),
        0,
        "empty replay_shapes means no catch-up replay"
    );

    ingest(&mut kernel, &"90".repeat(32), &author, 1_000);
    assert_eq!(
        capturing.count(),
        1,
        "a live-only observer (empty replay_shapes) must still be activated \
         via a fallback to the interest's own shape"
    );
}
