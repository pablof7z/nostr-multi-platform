use std::sync::Arc;

use nmp_core::slots::new_indexer_relays_slot;
use nmp_store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};
use nmp_core::substrate::{ExternalEventSinkPolicy, RawEventForwardPolicyContext};
use nmp_core::substrate::external_event_sink::{
    IngestOutcomeKind, SignedEventFrame, SinkDestination,
};
use nmp_core::RelayRole;

use super::IndexerRepublishPolicy;

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

fn context_with_indexers(urls: &[&str]) -> RawEventForwardPolicyContext {
    let slot = new_indexer_relays_slot();
    {
        let mut guard = slot.lock().expect("indexer slot");
        guard.replace(urls.iter().map(|url| (*url).to_string()).collect());
    }
    RawEventForwardPolicyContext::new(Arc::new(MemEventStore::new()), slot)
}

fn seed_store_with_provenance(store: &Arc<dyn EventStore>, raw: &RawEvent, source: &str) {
    let verified = VerifiedEvent::from_raw_unchecked(raw.clone());
    store
        .insert(verified, &source.to_string(), 1_700_000_000_000)
        .expect("seed insert");
}

/// Build a minimal `SignedEventFrame` from a `RawEvent` and optional source relay URL.
fn make_frame(raw: RawEvent, source: Option<&str>) -> SignedEventFrame {
    SignedEventFrame::build(
        Arc::new(raw),
        source.map(Arc::from),
        IngestOutcomeKind::Inserted,
    )
    .expect("frame build")
}

/// Extract relay URLs from destinations.
fn relay_urls(dests: &[SinkDestination]) -> Vec<String> {
    dests
        .iter()
        .filter_map(|d| match d {
            SinkDestination::Relay(t) => Some(t.relay_url.clone()),
        })
        .collect()
}

/// Assert all destinations are Relay with Indexer role.
fn all_indexer_role(dests: &[SinkDestination]) -> bool {
    dests.iter().all(|d| match d {
        SinkDestination::Relay(t) => t.relay_role == RelayRole::Indexer,
    })
}

#[test]
fn forwards_kind0_from_non_indexer_to_all_indexers() {
    let policy = IndexerRepublishPolicy::enabled(context_with_indexers(&[
        "wss://indexer-a/",
        "wss://indexer-b/",
    ]));
    let frame = make_frame(make_raw(0, 0x01), Some("wss://content-relay/"));

    let dests = policy.destinations(&frame);

    assert_eq!(dests.len(), 2);
    let urls = relay_urls(&dests);
    assert!(urls.contains(&"wss://indexer-a/".to_string()));
    assert!(urls.contains(&"wss://indexer-b/".to_string()));
    assert!(all_indexer_role(&dests));
}

#[test]
fn dedup_blocks_second_republish_of_same_event() {
    let policy = IndexerRepublishPolicy::enabled(context_with_indexers(&["wss://indexer/"]));
    let raw = make_raw(3, 0x02);

    let first = policy.destinations(&make_frame(raw.clone(), Some("wss://content-relay/")));
    let second = policy.destinations(&make_frame(raw, Some("wss://content-relay/")));

    assert_eq!(first.len(), 1);
    assert!(second.is_empty());
}

#[test]
fn skips_when_indexer_already_in_provenance() {
    let slot = new_indexer_relays_slot();
    {
        let mut guard = slot.lock().expect("indexer slot");
        guard.replace(vec!["wss://indexer/".to_string()]);
    }
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    let raw = make_raw(10_002, 0x03);
    seed_store_with_provenance(&store, &raw, "wss://indexer/");
    let context = RawEventForwardPolicyContext::new(store, slot);
    let policy = IndexerRepublishPolicy::enabled(context);

    let dests = policy.destinations(&make_frame(raw, Some("wss://content-relay/")));

    assert!(dests.is_empty());
}

#[test]
fn skips_when_source_is_an_indexer() {
    let policy = IndexerRepublishPolicy::enabled(context_with_indexers(&[
        "wss://indexer-a/",
        "wss://indexer-b/",
    ]));
    let frame = make_frame(make_raw(0, 0x04), Some("wss://indexer-a/"));

    let dests = policy.destinations(&frame);

    assert!(dests.is_empty());
}

#[test]
fn skips_non_replaceable_kinds() {
    let policy = IndexerRepublishPolicy::enabled(context_with_indexers(&["wss://indexer/"]));

    for kind in [1u32, 7, 5, 9_999, 20_000, 30_023, 40_000] {
        let frame = make_frame(
            make_raw(kind, 0x10 | (kind as u8 & 0x0f)),
            Some("wss://content-relay/"),
        );
        let dests = policy.destinations(&frame);
        assert!(
            dests.is_empty(),
            "non-replaceable kind {kind} must not forward"
        );
    }
}

#[test]
fn disabled_policy_is_a_noop() {
    let policy = IndexerRepublishPolicy::new(false, context_with_indexers(&["wss://indexer/"]));
    let frame = make_frame(make_raw(0, 0x05), Some("wss://content-relay/"));

    let dests = policy.destinations(&frame);

    assert!(dests.is_empty());
}

#[test]
fn empty_indexer_set_short_circuits() {
    let policy = IndexerRepublishPolicy::enabled(context_with_indexers(&[]));
    let frame = make_frame(make_raw(3, 0x06), Some("wss://content-relay/"));

    let dests = policy.destinations(&frame);

    assert!(dests.is_empty());
}

#[test]
fn different_indexers_are_independent_dedup_keys() {
    let policy = IndexerRepublishPolicy::enabled(context_with_indexers(&[
        "wss://indexer-a/",
        "wss://indexer-b/",
    ]));
    let frame = make_frame(make_raw(0, 0x07), Some("wss://content-relay/"));

    let dests = policy.destinations(&frame);

    assert_eq!(dests.len(), 2);
}
