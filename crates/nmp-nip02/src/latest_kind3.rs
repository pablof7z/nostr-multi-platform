//! Store-backed latest kind:3 follow-set reader.
//!
//! NIP-02 contact lists are replaceable events. The canonical follow set for
//! an author is therefore derived from that author's latest kind:3 event in the
//! kernel event store, not from a separate contacts cache.

use nmp_core::slots::{latest_kind3_follows_from_store, EventStoreSlot};

/// Latest kind:3 follow-set reader backed by the kernel-published event store.
#[derive(Clone)]
pub struct LatestKind3FollowSet {
    store: EventStoreSlot,
}

impl LatestKind3FollowSet {
    /// Build a reader over the kernel-published event-store slot.
    #[must_use]
    pub fn new(store: EventStoreSlot) -> Self {
        Self { store }
    }

    /// Resolve `author_hex`'s latest kind:3 follow set.
    ///
    /// Returns `None` when no store is published, the author is malformed, the
    /// store read fails, or no kind:3 exists. Returns `Some(vec![])` for an
    /// explicit latest kind:3 with no valid `p` tags.
    #[must_use]
    pub fn follows(&self, author_hex: &str) -> Option<Vec<String>> {
        latest_kind3_follows_from_store(&self.store, author_hex)
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;
    use nmp_core::slots::new_event_store_slot;
    use nmp_store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};
    use std::sync::Arc;

    pub(crate) fn reader_with_store() -> (LatestKind3FollowSet, Arc<dyn EventStore>) {
        let slot = new_event_store_slot();
        let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
        *slot.lock().expect("store slot") = Some(Arc::clone(&store));
        (LatestKind3FollowSet::new(slot), store)
    }

    pub(crate) fn insert_kind3(
        store: &Arc<dyn EventStore>,
        author: &str,
        event_id: &str,
        created_at: u64,
        follows: &[&str],
    ) {
        let tags = follows
            .iter()
            .map(|pk| vec!["p".to_string(), (*pk).to_string()])
            .collect();
        let raw = RawEvent {
            id: event_id_hex(event_id),
            pubkey: author.to_string(),
            created_at,
            kind: nmp_core::kinds::KIND_CONTACT_LIST,
            tags,
            content: String::new(),
            sig: "22".repeat(64),
        };
        let _ = store.insert(
            VerifiedEvent::from_raw_unchecked(raw),
            &"wss://store.test/".to_string(),
            created_at * 1000,
        );
    }

    fn event_id_hex(label: &str) -> String {
        if label.len() == 64 && label.bytes().all(|b| b.is_ascii_hexdigit()) {
            return label.to_string();
        }
        let mut out = String::new();
        for b in label.bytes().take(32) {
            out.push_str(&format!("{b:02x}"));
        }
        while out.len() < 64 {
            out.push('0');
        }
        out
    }
}
