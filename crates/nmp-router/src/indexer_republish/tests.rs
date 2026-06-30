use std::sync::Arc;

use nmp_core::slots::new_indexer_relays_slot;
use nmp_core::substrate::external_event_sink::{
    IngestOutcomeKind, SignedEventFrame, SinkDestination,
};
use nmp_core::substrate::{ExternalEventSinkPolicy, RawEventForwardPolicyContext};
use nmp_network::role::RelayRole;
use nmp_store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};

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

fn make_raw_with_d_tag(kind: u32, id_byte: u8, d_value: &str) -> RawEvent {
    let id = format!("{:02x}{}", id_byte, "00".repeat(31));
    RawEvent {
        id,
        pubkey: "11".repeat(32),
        created_at: 1_700_000_000,
        kind,
        tags: vec![vec!["d".to_string(), d_value.to_string()]],
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
fn skips_ephemeral_and_regular_non_replaceable_kinds() {
    let policy = IndexerRepublishPolicy::enabled(context_with_indexers(&["wss://indexer/"]));

    // Regular (non-replaceable), ephemeral, and above-addressable kinds must NOT forward.
    // kind:30023 is addressable and is covered by the positive test below.
    for kind in [1u32, 7, 5, 9_999, 20_000, 40_000] {
        let frame = make_frame(
            make_raw(kind, 0x10 | (kind as u8 & 0x0f)),
            Some("wss://content-relay/"),
        );
        let dests = policy.destinations(&frame);
        assert!(
            dests.is_empty(),
            "kind {kind} must not forward (not replaceable or addressable)"
        );
    }
}

#[test]
fn forwards_addressable_kind_to_indexer() {
    // kind:30023 = NIP-23 long-form; addressable / parameterized replaceable.
    let policy = IndexerRepublishPolicy::enabled(context_with_indexers(&[
        "wss://indexer-a/",
        "wss://indexer-b/",
    ]));
    let frame = make_frame(
        make_raw_with_d_tag(30_023, 0x20, "my-article"),
        Some("wss://content-relay/"),
    );

    let dests = policy.destinations(&frame);

    assert_eq!(
        dests.len(),
        2,
        "addressable kind 30023 must forward to all indexers"
    );
    let urls = relay_urls(&dests);
    assert!(urls.contains(&"wss://indexer-a/".to_string()));
    assert!(urls.contains(&"wss://indexer-b/".to_string()));
    assert!(all_indexer_role(&dests));
}

#[test]
fn addressable_kind_dedup_on_event_id_and_target() {
    // Dedup key is (event_id, target_relay_url) — two different addressable events
    // with the same kind/pubkey/d-tag are still independently forwarded.
    let policy = IndexerRepublishPolicy::enabled(context_with_indexers(&["wss://indexer/"]));

    let raw_a = make_raw_with_d_tag(30_000, 0x21, "follow-set");
    let raw_b = make_raw_with_d_tag(30_000, 0x22, "follow-set"); // same d-tag, different id

    let first = policy.destinations(&make_frame(raw_a.clone(), Some("wss://content-relay/")));
    let second_same = policy.destinations(&make_frame(raw_a, Some("wss://content-relay/")));
    let third_different = policy.destinations(&make_frame(raw_b, Some("wss://content-relay/")));

    assert_eq!(first.len(), 1, "first event must forward");
    assert!(second_same.is_empty(), "same event_id must be deduped");
    assert_eq!(
        third_different.len(),
        1,
        "different event_id must forward even if same d-tag"
    );
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
