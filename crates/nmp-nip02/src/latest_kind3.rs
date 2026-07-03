//! Store-backed latest kind:3 follow-set reader.
//!
//! NIP-02 contact lists are replaceable events. The canonical follow set for
//! an author is therefore derived from that author's latest kind:3 event in the
//! kernel event store, not from a separate contacts cache.

use nmp_core::slots::{ContactListEvent, ContactListReader, EventStoreSlot};
use nmp_store::{EventStore, StoredEvent};
use std::sync::Arc;

use crate::contact_tags::contact_follows;

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
        let store = self.store.lock().ok()?.clone()?;
        latest_contact_event(&store, author_hex).map(|stored| contact_follows(&stored.raw.tags))
    }
}

impl ContactListReader for LatestKind3FollowSet {
    fn follows(&self, author_hex: &str) -> Option<Vec<String>> {
        LatestKind3FollowSet::follows(self, author_hex)
    }

    fn event_for_edit(&self, author_hex: &str) -> Option<ContactListEvent> {
        let store = self.store.lock().ok()?.clone()?;
        latest_contact_event(&store, author_hex).map(|stored| ContactListEvent {
            tags: stored.raw.tags.clone(),
            content: stored.raw.content.clone(),
            created_at: stored.raw.created_at,
        })
    }
}

fn latest_contact_event(store: &Arc<dyn EventStore>, author_hex: &str) -> Option<StoredEvent> {
    let author = hex_to_pubkey_bytes(author_hex)?;
    let mut iter = store
        .scan_by_author_kind(
            &author,
            &[nmp_core::kinds::KIND_CONTACT_LIST],
            None,
            None,
            1,
        )
        .ok()?;
    iter.next()?.ok()
}

fn hex_to_pubkey_bytes(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0_u8; 32];
    for (idx, chunk) in hex.as_bytes().chunks_exact(2).enumerate() {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        out[idx] = (hi << 4) | lo;
    }
    Some(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
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

#[cfg(test)]
mod tests {
    use super::tests_support::{insert_kind3, reader_with_store};
    use super::*;

    const AUTHOR: &str = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
    const FOLLOW_A: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
    const FOLLOW_B: &str = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
    const ID_1: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const ID_2: &str = "2222222222222222222222222222222222222222222222222222222222222222";

    #[test]
    fn follows_reads_latest_replaceable_row() {
        let (reader, store) = reader_with_store();
        insert_kind3(&store, AUTHOR, ID_1, 100, &[FOLLOW_A]);
        insert_kind3(&store, AUTHOR, ID_2, 200, &[FOLLOW_B]);

        assert_eq!(reader.follows(AUTHOR), Some(vec![FOLLOW_B.to_string()]));
    }

    #[test]
    fn follows_returns_none_for_missing_contact_list() {
        let (reader, store) = reader_with_store();
        insert_kind3(&store, FOLLOW_A, ID_1, 100, &[FOLLOW_B]);

        assert_eq!(reader.follows(AUTHOR), None);
    }

    #[test]
    fn event_for_edit_returns_full_current_row() {
        let (reader, store) = reader_with_store();
        insert_kind3(&store, AUTHOR, ID_1, 100, &[FOLLOW_A]);

        let event = ContactListReader::event_for_edit(&reader, AUTHOR).expect("loaded");
        assert_eq!(event.created_at, 100);
        assert_eq!(event.content, "");
        assert_eq!(
            event.tags,
            vec![vec!["p".to_string(), FOLLOW_A.to_string()]]
        );
    }
}
