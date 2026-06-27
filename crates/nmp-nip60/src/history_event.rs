//! NIP-60 spending history event (kind:7376).
//!
//! Records each send/receive transaction. The encrypted content includes the
//! direction, amount, and references to created/destroyed token events.
//! Redeemed (nutzap) events are referenced in plain `e` tags (not encrypted).

use nostr::nips::nip44;
use nostr::{Event, EventBuilder, EventId, Keys, Kind, Tag};

use crate::error::Nip60Error;
use crate::kinds::KIND_NIP60_HISTORY;

/// Direction of a spending history entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    In,
    Out,
}

impl Direction {
    fn as_str(self) -> &'static str {
        match self {
            Self::In => "in",
            Self::Out => "out",
        }
    }
}

/// A spending history record corresponding to a kind:7376 event.
#[derive(Debug, Clone)]
pub struct HistoryRecord {
    pub direction: Direction,
    pub amount: u64,
    /// Token event IDs that were created in this transaction.
    pub created: Vec<EventId>,
    /// Token event IDs that were destroyed in this transaction.
    pub destroyed: Vec<EventId>,
    /// Nutzap event IDs that were redeemed — stored plain.
    pub redeemed: Vec<EventId>,
}

impl HistoryRecord {
    pub fn new_in(amount: u64) -> Self {
        Self {
            direction: Direction::In,
            amount,
            created: Vec::new(),
            destroyed: Vec::new(),
            redeemed: Vec::new(),
        }
    }

    pub fn new_out(amount: u64) -> Self {
        Self {
            direction: Direction::Out,
            amount,
            created: Vec::new(),
            destroyed: Vec::new(),
            redeemed: Vec::new(),
        }
    }
}

// ─── Encode ────────────────────────────────────────────────────────────────

/// Build a kind:7376 spending history event.
pub fn build_history_event(record: &HistoryRecord, keys: &Keys) -> Result<EventBuilder, Nip60Error> {
    let mut data: Vec<Vec<String>> = vec![
        vec!["direction".into(), record.direction.as_str().into()],
        vec!["amount".into(), record.amount.to_string()],
    ];
    for id in &record.created {
        data.push(vec!["e".into(), id.to_hex(), String::new(), "created".into()]);
    }
    for id in &record.destroyed {
        data.push(vec!["e".into(), id.to_hex(), String::new(), "destroyed".into()]);
    }

    let json = serde_json::to_string(&data)?;
    let content =
        nip44::encrypt(keys.secret_key(), &keys.public_key(), json, nip44::Version::V2)
            .map_err(|e| Nip60Error::Nip44(format!("{e}")))?;

    // Redeemed events go in plain tags (not encrypted).
    let mut tags = Vec::new();
    for id in &record.redeemed {
        tags.push(Tag::parse(["e", &id.to_hex(), "", "redeemed"]).map_err(|e| {
            Nip60Error::Event(format!("history redeemed tag: {e}"))
        })?);
    }

    Ok(EventBuilder::new(Kind::from(KIND_NIP60_HISTORY as u16), content).tags(tags))
}

/// Extract kind:7376 plain `e` tags marked as redeemed nutzap receipts.
pub fn redeemed_nutzap_ids(event: &Event) -> Vec<EventId> {
    if event.kind != Kind::from(KIND_NIP60_HISTORY as u16) {
        return Vec::new();
    }

    event
        .tags
        .iter()
        .filter_map(|tag| {
            let row = tag.as_slice();
            match (
                row.first().map(String::as_str),
                row.get(1),
                row.get(3).map(String::as_str),
            ) {
                (Some("e"), Some(id), Some("redeemed")) => EventId::from_hex(id).ok(),
                _ => None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redeemed_nutzap_ids_reads_plain_history_tags() {
        let keys = Keys::generate();
        let redeemed = EventId::from_byte_array([7u8; 32]);
        let ignored_created = EventId::from_byte_array([9u8; 32]);
        let mut history = HistoryRecord::new_in(10);
        history.redeemed.push(redeemed);
        history.created.push(ignored_created);

        let event = build_history_event(&history, &keys)
            .expect("history event builder")
            .sign_with_keys(&keys)
            .expect("signed history event");

        assert_eq!(redeemed_nutzap_ids(&event), vec![redeemed]);
    }
}
