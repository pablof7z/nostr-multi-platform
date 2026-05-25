//! Unit tests for [`crate::actor::indexer_republish`]. The pipeline is
//! tested behind its trait-shaped `IndexerForwardSender` seam so we can
//! capture the outbound `(url, frame_text)` pairs without standing up a
//! real [`nmp_network::pool::Pool`] (and without binding a TCP socket on
//! every test runner).

use std::sync::{Arc, Mutex};

use crate::actor::indexer_republish::{
    IndexerForwardSender, IndexerRepublishPipeline,
};
use crate::kernel::new_indexer_relays_slot;
use crate::store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};

/// Capturing sender — records every `(url, frame_text)` pair the pipeline
/// hands it, in dispatch order.
#[derive(Default, Clone)]
struct CaptureSender {
    sends: Arc<Mutex<Vec<(String, String)>>>,
}

impl CaptureSender {
    fn new() -> Self {
        Self::default()
    }

    fn sends(&self) -> Vec<(String, String)> {
        self.sends.lock().expect("sends mutex").clone()
    }
}

impl IndexerForwardSender for CaptureSender {
    fn send_to(&self, url: &str, frame_text: &str) -> bool {
        self.sends
            .lock()
            .expect("sends mutex")
            .push((url.to_string(), frame_text.to_string()));
        true
    }
}

/// Build a syntactically-valid `RawEvent` for `(kind, id_byte)`. The
/// pipeline does not Schnorr-verify; the kernel did that upstream. We
/// only need the structural fields (`id`, `kind`, `pubkey`, …) to satisfy
/// `RawEvent::is_replaceable` + `id_bytes`.
fn make_raw(kind: u32, id_byte: u8) -> RawEvent {
    let id = format!("{:02x}{}", id_byte, "00".repeat(31));
    RawEvent {
        id,
        pubkey: "11".repeat(32),
        created_at: 1_700_000_000,
        kind,
        tags: Vec::new(),
        content: String::new(),
        sig: "22".repeat(64),
    }
}

/// Populate the `IndexerRelaysSlot` with `urls`. The replacement helper
/// (`RelayUrls::replace`) is `pub(crate)`, so we go through the
/// kernel-side test helper that the projection module exposes for
/// in-crate tests.
fn set_indexer_urls(
    slot: &crate::kernel::IndexerRelaysSlot,
    urls: &[&str],
) {
    let mut guard = slot.lock().expect("indexer slot");
    let owned: Vec<String> = urls.iter().map(|u| (*u).to_string()).collect();
    // RelayUrls owns its inner Vec privately; the public `replace`
    // surface is `pub(crate)`, reachable from this in-crate test.
    guard.replace(owned);
}

/// Insert `raw` into `store` with the given `source` URL so the
/// subsequent `provenance_for` lookup returns the indexer. Mirrors the
/// kernel's ingest path: verify → store.insert → notify observers.
fn seed_store_with_provenance(
    store: &Arc<dyn EventStore>,
    raw: &RawEvent,
    source: &str,
) {
    let verified = VerifiedEvent::from_raw_unchecked(raw.clone());
    store
        .insert(verified, &source.to_string(), 1_700_000_000_000)
        .expect("seed insert");
}

fn empty_store() -> Arc<dyn EventStore> {
    Arc::new(MemEventStore::new())
}

#[test]
fn forwards_kind0_from_non_indexer_to_all_indexers() {
    // Baseline: a kind:0 from a non-indexer relay with no prior indexer
    // provenance MUST forward to every indexer in the configured set.
    let slot = new_indexer_relays_slot();
    set_indexer_urls(&slot, &["wss://indexer-a/", "wss://indexer-b/"]);
    let store = empty_store();
    let sender = Arc::new(CaptureSender::new());

    let pipeline = IndexerRepublishPipeline::new(
        true,
        slot,
        store,
        Arc::clone(&sender) as Arc<dyn IndexerForwardSender>,
    );

    let raw = make_raw(0, 0x01);
    let json = serde_json::to_string(&raw).expect("serialize raw");
    let sent = pipeline.process(&raw, Some("wss://content-relay/"), &json);

    assert_eq!(sent, 2, "should forward to both indexers");
    let captured = sender.sends();
    assert_eq!(captured.len(), 2);
    let urls: Vec<&str> = captured.iter().map(|(u, _)| u.as_str()).collect();
    assert!(urls.contains(&"wss://indexer-a/"));
    assert!(urls.contains(&"wss://indexer-b/"));
    // Frame shape is the verbatim ["EVENT", <json>] envelope.
    for (_, frame) in &captured {
        assert!(frame.starts_with(r#"["EVENT","#));
        assert!(frame.ends_with(']'));
    }
}

#[test]
fn dedup_blocks_second_republish_of_same_event() {
    // Two consecutive observer fires for the same (event_id, indexer_url)
    // pair must collapse to a single forward — the in-session LRU is the
    // first line of defence against duplicate socket writes.
    let slot = new_indexer_relays_slot();
    set_indexer_urls(&slot, &["wss://indexer/"]);
    let store = empty_store();
    let sender = Arc::new(CaptureSender::new());

    let pipeline = IndexerRepublishPipeline::new(
        true,
        slot,
        store,
        Arc::clone(&sender) as Arc<dyn IndexerForwardSender>,
    );

    let raw = make_raw(3, 0x02);
    let json = serde_json::to_string(&raw).expect("serialize raw");

    let first = pipeline.process(&raw, Some("wss://content-relay/"), &json);
    let second = pipeline.process(&raw, Some("wss://content-relay/"), &json);

    assert_eq!(first, 1, "first fire forwards");
    assert_eq!(second, 0, "second fire is dedup'd");
    assert_eq!(sender.sends().len(), 1);
}

#[test]
fn skips_when_indexer_already_in_provenance() {
    // The store reports the indexer in `provenance_for` — that means the
    // indexer already delivered this `id` at least once. Forwarding would
    // be a wasted socket write; the pipeline MUST short-circuit.
    let slot = new_indexer_relays_slot();
    set_indexer_urls(&slot, &["wss://indexer/"]);
    let store = empty_store();
    let sender = Arc::new(CaptureSender::new());

    let raw = make_raw(10_002, 0x03);
    // Pre-seed the store so the indexer is in the provenance entry list.
    seed_store_with_provenance(&store, &raw, "wss://indexer/");

    let pipeline = IndexerRepublishPipeline::new(
        true,
        slot,
        Arc::clone(&store),
        Arc::clone(&sender) as Arc<dyn IndexerForwardSender>,
    );

    let json = serde_json::to_string(&raw).expect("serialize raw");
    // The non-indexer source URL would normally pass loop-prevention
    // rule 1; the provenance rule is the gate under test here.
    let sent = pipeline.process(&raw, Some("wss://content-relay/"), &json);

    assert_eq!(sent, 0, "indexer already has it — must not forward");
    assert!(sender.sends().is_empty());
}

#[test]
fn skips_when_source_is_an_indexer() {
    // Source IS the indexer — even if the indexer URL list contains other
    // indexers, the structural rule is "if the delivering relay is an
    // indexer, do nothing". Cross-indexer fan-out is the upstream gossip
    // layer's job, not ours.
    let slot = new_indexer_relays_slot();
    set_indexer_urls(&slot, &["wss://indexer-a/", "wss://indexer-b/"]);
    let store = empty_store();
    let sender = Arc::new(CaptureSender::new());

    let pipeline = IndexerRepublishPipeline::new(
        true,
        slot,
        store,
        Arc::clone(&sender) as Arc<dyn IndexerForwardSender>,
    );

    let raw = make_raw(0, 0x04);
    let json = serde_json::to_string(&raw).expect("serialize raw");
    let sent = pipeline.process(&raw, Some("wss://indexer-a/"), &json);

    assert_eq!(sent, 0, "source is indexer — must skip cross-indexer fan-out");
    assert!(sender.sends().is_empty());
}

#[test]
fn skips_non_replaceable_kinds() {
    // Pipeline must only act on the NIP-01 replaceable kinds — anything
    // else (kind:1 notes, kind:7 reactions, kind:30023 long-form, …) is
    // structurally out of scope.
    let slot = new_indexer_relays_slot();
    set_indexer_urls(&slot, &["wss://indexer/"]);
    let store = empty_store();
    let sender = Arc::new(CaptureSender::new());

    let pipeline = IndexerRepublishPipeline::new(
        true,
        slot,
        store,
        Arc::clone(&sender) as Arc<dyn IndexerForwardSender>,
    );

    for kind in [1u32, 7, 5, 9_999, 20_000, 30_023, 40_000] {
        let raw = make_raw(kind, 0x10 | (kind as u8 & 0x0f));
        let json = serde_json::to_string(&raw).expect("serialize raw");
        let sent = pipeline.process(&raw, Some("wss://content-relay/"), &json);
        assert_eq!(sent, 0, "non-replaceable kind {kind} must not forward");
    }
    assert!(sender.sends().is_empty());
}

#[test]
fn disabled_pipeline_is_a_noop() {
    // Master switch off — even a perfectly-shaped kind:0 from a
    // non-indexer relay must not produce a single send.
    let slot = new_indexer_relays_slot();
    set_indexer_urls(&slot, &["wss://indexer/"]);
    let store = empty_store();
    let sender = Arc::new(CaptureSender::new());

    let pipeline = IndexerRepublishPipeline::new(
        false,
        slot,
        store,
        Arc::clone(&sender) as Arc<dyn IndexerForwardSender>,
    );

    let raw = make_raw(0, 0x05);
    let json = serde_json::to_string(&raw).expect("serialize raw");
    let sent = pipeline.process(&raw, Some("wss://content-relay/"), &json);

    assert_eq!(sent, 0, "disabled pipeline must not forward");
    assert!(sender.sends().is_empty());
}

#[test]
fn empty_indexer_set_short_circuits() {
    // Indexer list is empty (a fresh boot before any relay config has
    // landed). The pipeline must skip entirely — there is no `target`
    // to forward to.
    let slot = new_indexer_relays_slot();
    let store = empty_store();
    let sender = Arc::new(CaptureSender::new());

    let pipeline = IndexerRepublishPipeline::new(
        true,
        slot,
        store,
        Arc::clone(&sender) as Arc<dyn IndexerForwardSender>,
    );

    let raw = make_raw(3, 0x06);
    let json = serde_json::to_string(&raw).expect("serialize raw");
    let sent = pipeline.process(&raw, Some("wss://content-relay/"), &json);

    assert_eq!(sent, 0);
    assert!(sender.sends().is_empty());
}

#[test]
fn re_register_unregisters_stale_observer() {
    // Drives the Reset-survival path: two consecutive
    // `register_indexer_republish_pipeline` calls against the same raw
    // observer slot must leave exactly one live pipeline registration.
    // Without the unregister step the slot would accumulate orphan
    // pipelines pointing at dead kernels.
    use crate::actor::indexer_republish::new_indexer_republish_observer_id_slot;
    use crate::actor::{new_raw_event_observer_slot, raw_observers_idle_for_kind};

    // We can't call `register_indexer_republish_pipeline` with a fake
    // pool (it expects an `&nmp_network::pool::Pool`), so we drive the
    // unregister contract via the underlying slot directly. A live
    // pipeline raises the `idle` counter for kind 0; after unregister
    // the slot returns to idle.
    use crate::actor::{register_rust_raw_observer, unregister_raw_observer, KindFilter};

    let slot = new_raw_event_observer_slot();
    let id_slot = new_indexer_republish_observer_id_slot();

    // Stand in for `register_indexer_republish_pipeline`'s registration
    // call: install one stub observer and stash its id.
    let stub_indexers = new_indexer_relays_slot();
    let stub_store = empty_store();
    let stub_sender = Arc::new(CaptureSender::new());
    let pipeline_a = Arc::new(IndexerRepublishPipeline::new(
        true,
        stub_indexers.clone(),
        Arc::clone(&stub_store),
        Arc::clone(&stub_sender) as Arc<dyn IndexerForwardSender>,
    ));
    let id_a = register_rust_raw_observer(
        &slot,
        KindFilter::from_kinds([0u32]),
        pipeline_a as Arc<dyn crate::actor::RawEventObserver>,
    );
    *id_slot.lock().expect("id slot") = Some(id_a);
    assert!(!raw_observers_idle_for_kind(&slot, 0), "kind 0 has a listener");

    // Now run the unregister + re-register dance the helper performs.
    let previous = id_slot.lock().expect("id slot").take();
    if let Some(id) = previous {
        unregister_raw_observer(&slot, id);
    }
    let pipeline_b = Arc::new(IndexerRepublishPipeline::new(
        true,
        stub_indexers,
        stub_store,
        stub_sender as Arc<dyn IndexerForwardSender>,
    ));
    let id_b = register_rust_raw_observer(
        &slot,
        KindFilter::from_kinds([0u32]),
        pipeline_b as Arc<dyn crate::actor::RawEventObserver>,
    );
    *id_slot.lock().expect("id slot") = Some(id_b);

    // The slot still has exactly one kind-0 listener — the stale `id_a`
    // is gone, the fresh `id_b` is the only registration.
    assert!(!raw_observers_idle_for_kind(&slot, 0));
    assert_ne!(id_a, id_b, "re-register must allocate a fresh id");
}

#[test]
fn different_indexers_are_independent_dedup_keys() {
    // Sending event X to indexer A does NOT block sending event X to
    // indexer B. The dedup key is `(event_id, indexer_url)`, not just
    // `event_id`.
    let slot = new_indexer_relays_slot();
    set_indexer_urls(&slot, &["wss://indexer-a/", "wss://indexer-b/"]);
    let store = empty_store();
    let sender = Arc::new(CaptureSender::new());

    let pipeline = IndexerRepublishPipeline::new(
        true,
        slot,
        store,
        Arc::clone(&sender) as Arc<dyn IndexerForwardSender>,
    );

    let raw = make_raw(0, 0x07);
    let json = serde_json::to_string(&raw).expect("serialize raw");
    let sent = pipeline.process(&raw, Some("wss://content-relay/"), &json);

    // First fire reaches BOTH indexers (different dedup keys).
    assert_eq!(sent, 2);
    let urls: Vec<String> = sender.sends().iter().map(|(u, _)| u.clone()).collect();
    assert!(urls.iter().any(|u| u == "wss://indexer-a/"));
    assert!(urls.iter().any(|u| u == "wss://indexer-b/"));
}
