use std::sync::Arc;

use nmp_core::slots::new_indexer_relays_slot;
use nmp_core::store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};
use nmp_core::substrate::{RawEventForwardPolicy, RawEventForwardPolicyContext};
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

#[test]
fn forwards_kind0_from_non_indexer_to_all_indexers() {
    let policy = IndexerRepublishPolicy::enabled(context_with_indexers(&[
        "wss://indexer-a/",
        "wss://indexer-b/",
    ]));
    let raw = make_raw(0, 0x01);

    let targets = policy.forward_targets(&raw, Some("wss://content-relay/"));

    assert_eq!(targets.len(), 2);
    assert!(targets
        .iter()
        .any(|target| target.relay_url == "wss://indexer-a/"));
    assert!(targets
        .iter()
        .any(|target| target.relay_url == "wss://indexer-b/"));
    assert!(targets
        .iter()
        .all(|target| target.relay_role == RelayRole::Indexer));
}

#[test]
fn dedup_blocks_second_republish_of_same_event() {
    let policy = IndexerRepublishPolicy::enabled(context_with_indexers(&["wss://indexer/"]));
    let raw = make_raw(3, 0x02);

    let first = policy.forward_targets(&raw, Some("wss://content-relay/"));
    let second = policy.forward_targets(&raw, Some("wss://content-relay/"));

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

    let targets = policy.forward_targets(&raw, Some("wss://content-relay/"));

    assert!(targets.is_empty());
}

#[test]
fn skips_when_source_is_an_indexer() {
    let policy = IndexerRepublishPolicy::enabled(context_with_indexers(&[
        "wss://indexer-a/",
        "wss://indexer-b/",
    ]));
    let raw = make_raw(0, 0x04);

    let targets = policy.forward_targets(&raw, Some("wss://indexer-a/"));

    assert!(targets.is_empty());
}

#[test]
fn skips_non_replaceable_kinds() {
    let policy = IndexerRepublishPolicy::enabled(context_with_indexers(&["wss://indexer/"]));

    for kind in [1u32, 7, 5, 9_999, 20_000, 30_023, 40_000] {
        let raw = make_raw(kind, 0x10 | (kind as u8 & 0x0f));
        let targets = policy.forward_targets(&raw, Some("wss://content-relay/"));
        assert!(
            targets.is_empty(),
            "non-replaceable kind {kind} must not forward"
        );
    }
}

#[test]
fn disabled_policy_is_a_noop() {
    let policy = IndexerRepublishPolicy::new(false, context_with_indexers(&["wss://indexer/"]));
    let raw = make_raw(0, 0x05);

    let targets = policy.forward_targets(&raw, Some("wss://content-relay/"));

    assert!(targets.is_empty());
}

#[test]
fn empty_indexer_set_short_circuits() {
    let policy = IndexerRepublishPolicy::enabled(context_with_indexers(&[]));
    let raw = make_raw(3, 0x06);

    let targets = policy.forward_targets(&raw, Some("wss://content-relay/"));

    assert!(targets.is_empty());
}

#[test]
fn different_indexers_are_independent_dedup_keys() {
    let policy = IndexerRepublishPolicy::enabled(context_with_indexers(&[
        "wss://indexer-a/",
        "wss://indexer-b/",
    ]));
    let raw = make_raw(0, 0x07);

    let targets = policy.forward_targets(&raw, Some("wss://content-relay/"));

    assert_eq!(targets.len(), 2);
}
