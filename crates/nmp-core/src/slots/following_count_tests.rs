use super::*;
use crate::store::EventStore;
use std::sync::Arc;

fn slot_with(store: nmp_store::MemEventStore) -> EventStoreSlot {
    let slot = new_event_store_slot();
    *slot.lock().unwrap() = Some(Arc::new(store) as Arc<dyn crate::store::EventStore>);
    slot
}

fn kind3(author: &str, follows: &[&str]) -> nmp_store::VerifiedEvent {
    let mut tags: Vec<Vec<String>> = follows
        .iter()
        .map(|p| vec!["p".to_string(), (*p).to_string()])
        .collect();
    // A non-`p` tag and a malformed `p` must not be counted.
    tags.push(vec!["t".to_string(), "noise".to_string()]);
    tags.push(vec!["p".to_string(), "not-hex".to_string()]);
    let raw = nmp_store::RawEvent {
        id: "ab".repeat(32),
        pubkey: author.to_string(),
        created_at: 1_700_000_000,
        kind: 3,
        tags,
        content: String::new(),
        sig: "cd".repeat(64),
    };
    nmp_store::VerifiedEvent::from_raw_unchecked(raw)
}

#[test]
fn counts_distinct_hex_p_tags_in_latest_kind3() {
    let author = "11".repeat(32);
    let a = "22".repeat(32);
    let b = "33".repeat(32);
    let store = nmp_store::MemEventStore::default();
    // Duplicate `a` must be counted once.
    let _ = store.insert(kind3(&author, &[&a, &b, &a]), &"wss://r".to_string(), 1);
    let slot = slot_with(store);
    assert_eq!(following_count_from_store(&slot, &author), Some(2));
}

#[test]
fn none_when_no_kind3_for_author() {
    let author = "11".repeat(32);
    let store = nmp_store::MemEventStore::default();
    let slot = slot_with(store);
    assert_eq!(following_count_from_store(&slot, &author), None);
}

#[test]
fn none_when_author_hex_malformed_or_slot_empty() {
    let slot = new_event_store_slot();
    assert_eq!(following_count_from_store(&slot, "11".repeat(32).as_str()), None);
    let store = nmp_store::MemEventStore::default();
    let slot = slot_with(store);
    assert_eq!(following_count_from_store(&slot, "not-hex"), None);
}
