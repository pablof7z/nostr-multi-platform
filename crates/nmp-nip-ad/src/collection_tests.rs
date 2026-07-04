//! Unit + harness tests for the NIP-AD collection doorway.
//!
//! Driving a full live session end-to-end is out of scope at this layer: a real
//! `ReadHost` is an app runtime, and this concept crate must not depend on a
//! runtime crate (read-door doctrine, #2777). So the delivery path is proven
//! with a fake `ReadHost` that captures the observer + typed-output encoder and
//! feeds an event through, then decodes the `ADCL` snapshot. The live
//! network path (resolve trellis.rs → on-wire kind:30023 + d-tag) is covered by
//! `tests/live_trellis.rs`, which now also drives the matched event through
//! `open_ad_collection`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{KernelEvent, ObservedProjection};
use nmp_core::{ObservedProjectionId, ObservedProjectionSink};
use nmp_ownership::ProjectionRegistrationKey;
use nmp_read_session::{
    InterestLifecycle, ReadHost, ReadOutputEncoder, ReadReplayPolicy, ReadSessionBuild,
    ReadSessionId, ReadSessionRegistry, TeardownAction,
};

use super::*;
use crate::decode_ad_collection_snapshot;

// ── fixtures ────────────────────────────────────────────────────────────────

fn resolution(filter_json: &str, relays: &[&str]) -> AdResolution {
    let filter: nostr::Filter = serde_json::from_str(filter_json).expect("valid nostr filter");
    AdResolution {
        filter,
        relays: relays.iter().map(|s| (*s).to_string()).collect(),
    }
}

fn event(id: &str, kind: u32, created_at: u64, tags: Vec<Vec<String>>) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: "author-pubkey".to_string(),
        kind,
        created_at,
        tags,
        content: format!("content-{id}"),
        relay_provenance: vec!["wss://origin.example".to_string()],
    }
}

/// A minimal `ReadHost` that captures the read's observer + typed-output encoder
/// so a test can feed events through the real doorway and decode the snapshot.
struct FakeHost {
    registry: ReadSessionRegistry,
    observer: Arc<Mutex<Option<Arc<dyn ObservedProjectionSink>>>>,
    encoder: Arc<Mutex<Option<ReadOutputEncoder>>>,
    next_interest: Arc<AtomicU64>,
    opened: Arc<Mutex<Vec<String>>>,
}

impl FakeHost {
    fn new() -> Self {
        Self {
            registry: ReadSessionRegistry::default(),
            observer: Arc::new(Mutex::new(None)),
            encoder: Arc::new(Mutex::new(None)),
            next_interest: Arc::new(AtomicU64::new(1)),
            opened: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn feed(&self, ev: &KernelEvent) {
        let observer = self.observer.lock().unwrap().clone();
        if let Some(observer) = observer {
            observer.on_kernel_event(ev);
        }
    }

    fn snapshot(&self) -> AdCollectionSnapshot {
        let guard = self.encoder.lock().unwrap();
        let encoder = guard.as_ref().expect("output installed");
        let data = encoder().expect("encoder yields typed data");
        decode_ad_collection_snapshot(&data.payload).expect("valid ADCL payload")
    }

    fn opened_filters(&self) -> Vec<String> {
        self.opened.lock().unwrap().clone()
    }
}

impl ReadHost for FakeHost {
    fn install_read_output(&self, _key: ProjectionRegistrationKey, encoder: ReadOutputEncoder) {
        *self.encoder.lock().unwrap() = Some(encoder);
    }
    fn open_read_interest(&self, decl: ObservedProjection) -> ObservedProjectionId {
        *self.observer.lock().unwrap() = Some(Arc::clone(&decl.observer));
        self.opened.lock().unwrap().push(decl.filter_json.clone());
        ObservedProjectionId(self.next_interest.fetch_add(1, Ordering::Relaxed))
    }
    fn teardown_close_interest(&self, _id: ObservedProjectionId) -> TeardownAction {
        Box::new(|| {})
    }
    fn teardown_remove_output(&self, _key: String) -> TeardownAction {
        Box::new(|| {})
    }
    fn teardown_mark_changed(&self) -> TeardownAction {
        Box::new(|| {})
    }
    fn store_read_session(&self, build: ReadSessionBuild) -> ReadSessionId {
        self.registry.open(build)
    }
    fn read_session_projection_key(&self, id: &ReadSessionId) -> Option<String> {
        self.registry.projection_key(id)
    }
    fn close_read_session(&self, id: &ReadSessionId) -> bool {
        self.registry.close(id)
    }
    fn close_read_session_by_projection_key(&self, projection_key: &str) -> bool {
        self.registry.close_by_projection_key(projection_key)
    }
}

// ── demand shape ──────────────────────────────────────────────────────────

#[test]
fn demands_are_one_shot_relay_pinned_per_relay() {
    let res = resolution(
        r##"{"kinds":[30023],"#d":["legible"]}"##,
        &["wss://relay-a.example", "wss://relay-b.example"],
    );
    let demands = ad_collection_demands(&res, "sess");

    assert_eq!(demands.len(), 2, "one demand per resolved relay");
    for (demand, relay) in demands
        .iter()
        .zip(["wss://relay-a.example", "wss://relay-b.example"])
    {
        assert_eq!(
            demand.lifecycle,
            InterestLifecycle::OneShot,
            "an AD collection demand must CLOSE on EOSE"
        );
        assert_eq!(demand.relay_pin.as_deref(), Some(relay), "pinned per relay");
        assert_eq!(demand.scope, 1, "Global scope");
        assert!(!demand.is_indexer_discovery);
        assert!(matches!(demand.replay, ReadReplayPolicy::Structural));
        assert!(
            demand.filter_json.contains("30023"),
            "filter carries the resolved kind; got {}",
            demand.filter_json
        );
        assert!(
            demand.consumer_id.contains("sess") && demand.consumer_id.contains(relay),
            "consumer key is per-session-per-relay; got {}",
            demand.consumer_id
        );
    }
}

// ── reducer: dedupe + order ─────────────────────────────────────────────────

#[test]
fn projection_dedupes_by_id_and_orders_newest_first() {
    let mut projection = AdCollectionProjection::new();
    projection.ingest_relay_event(&event("id-a", 20, 100, vec![]), "wss://a".to_string());
    projection.ingest_relay_event(&event("id-b", 20, 300, vec![]), "wss://a".to_string());
    projection.ingest_relay_event(&event("id-c", 20, 200, vec![]), "wss://a".to_string());
    // Duplicate id (first arrival wins → the created_at:100 row is kept).
    projection.ingest_relay_event(&event("id-a", 20, 999, vec![]), "wss://b".to_string());

    assert_eq!(projection.len(), 3, "dedupe by id");
    let snap = projection.snapshot();
    assert_eq!(
        snap.rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        vec!["id-b", "id-c", "id-a"],
        "ordered created_at desc"
    );
    assert_eq!(
        snap.rows.last().unwrap().created_at,
        100,
        "first arrival wins on a duplicate id"
    );
}

// ── doorway delivery through a fake host ─────────────────────────────────────

#[test]
fn open_ad_collection_delivers_events_into_the_typed_snapshot() {
    let host = FakeHost::new();
    let res = resolution(
        r##"{"kinds":[30023],"#d":["the-machine-that-could-tell-you-why"]}"##,
        &["wss://relay.primal.net"],
    );

    let handle = open_ad_collection(&host, &res, "trellis-legible");
    assert_eq!(
        handle.projection_key(),
        "nmp.nip-ad.collection.trellis-legible"
    );
    assert_eq!(
        host.opened_filters().len(),
        1,
        "one pinned demand opened for the one relay"
    );

    // Multi-result is first-class: feed two distinct kind:30023 events.
    host.feed(&event(
        "article-1",
        30023,
        1000,
        vec![vec![
            "d".to_string(),
            "the-machine-that-could-tell-you-why".to_string(),
        ]],
    ));
    host.feed(&event("article-2", 30023, 2000, vec![]));

    let snap = host.snapshot();
    assert_eq!(snap.rows.len(), 2, "both delivered events land, newest-first");
    assert_eq!(snap.rows[0].id, "article-2");
    assert_eq!(snap.rows[0].kind, 30023);
    // The d-tag rides through verbatim for the downstream kind:30023 renderer.
    let article_1 = snap.rows.iter().find(|r| r.id == "article-1").unwrap();
    assert!(
        article_1.tags.iter().any(|t| {
            t.first().map(String::as_str) == Some("d")
                && t.get(1).map(String::as_str) == Some("the-machine-that-could-tell-you-why")
        }),
        "the resolved d tag must survive into the typed row"
    );
}

// ── fail-open: empty relays ──────────────────────────────────────────────────

#[test]
fn empty_relays_is_fail_open_empty_snapshot_not_error() {
    let host = FakeHost::new();
    let res = resolution(r#"{"kinds":[20]}"#, &[]);

    let handle = open_ad_collection(&host, &res, "no-relays");

    assert!(
        host.opened_filters().is_empty(),
        "no relays → no demands opened"
    );
    assert_eq!(handle.projection_key(), "nmp.nip-ad.collection.no-relays");
    // The typed output is still installed; the snapshot is simply empty.
    assert!(
        host.snapshot().rows.is_empty(),
        "an empty resolution yields an empty collection, not an error"
    );
}
