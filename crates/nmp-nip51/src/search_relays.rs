//! Active account NIP-51 kind:10007 search-relay facts.
//!
//! This module owns the NIP-51 parsing for search relays. The generic planner
//! and router consume relay facts; they do not parse kind:10007 events.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::KernelEvent;
use nmp_core::{canonical_relay_url, KernelEventObserver};
use nmp_kinds::KIND_SEARCH_RELAYS;
use serde::Serialize;

/// Snapshot shape for the active account's preferred NIP-50 relays.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct SearchRelayListSnapshot {
    pub relays: Vec<String>,
}

#[derive(Default)]
struct SearchRelaySet {
    owner_pubkey: Option<String>,
    created_at: u64,
    relays: BTreeSet<String>,
}

/// Projects the active account's kind:10007 relay list.
pub struct SearchRelayListProjection {
    active_pubkey: Arc<Mutex<Option<String>>>,
    relays: Mutex<SearchRelaySet>,
}

impl SearchRelayListProjection {
    #[must_use]
    pub fn new(active_pubkey: Arc<Mutex<Option<String>>>) -> Self {
        Self {
            active_pubkey,
            relays: Mutex::new(SearchRelaySet::default()),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> SearchRelayListSnapshot {
        let active = match self.active_pubkey.lock() {
            Ok(guard) => guard.as_ref().cloned(),
            Err(_) => return SearchRelayListSnapshot::default(),
        };
        let Ok(relays) = self.relays.lock() else {
            return SearchRelayListSnapshot::default();
        };
        if relays.owner_pubkey.as_deref() != active.as_deref() {
            return SearchRelayListSnapshot::default();
        }
        SearchRelayListSnapshot {
            relays: relays.relays.iter().cloned().collect(),
        }
    }

    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot())
            .unwrap_or_else(|_| serde_json::json!({ "relays": [] }))
    }
}

impl KernelEventObserver for SearchRelayListProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind != KIND_SEARCH_RELAYS {
            return;
        }

        let active = match self.active_pubkey.lock() {
            Ok(guard) => guard.as_ref().cloned(),
            Err(_) => return,
        };
        if active.as_deref() != Some(event.author.as_str()) {
            return;
        }

        let relays: BTreeSet<String> = event
            .tags
            .iter()
            .filter_map(|tag| {
                let url = tag
                    .first()
                    .is_some_and(|t| t == "relay")
                    .then(|| tag.get(1))??;
                canonical_relay_url(url)
            })
            .collect();

        let Ok(mut state) = self.relays.lock() else {
            return;
        };
        if state.owner_pubkey.as_deref() == Some(event.author.as_str())
            && event.created_at < state.created_at
        {
            return;
        }
        *state = SearchRelaySet {
            owner_pubkey: Some(event.author.clone()),
            created_at: event.created_at,
            relays,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::EventId;

    const ALICE: &str = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
    const BOB: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";

    fn projection_for(active: Option<&str>) -> SearchRelayListProjection {
        SearchRelayListProjection::new(Arc::new(Mutex::new(active.map(str::to_string))))
    }

    fn event(author: &str, created_at: u64, kind: u32, tags: Vec<Vec<&str>>) -> KernelEvent {
        KernelEvent {
            id: EventId::from(
                "0000000000000000000000000000000000000000000000000000000000000001".to_string(),
            ),
            author: author.to_string(),
            kind,
            created_at,
            tags: tags
                .into_iter()
                .map(|tag| tag.into_iter().map(str::to_string).collect())
                .collect(),
            content: String::new(),
            relay_provenance: Vec::new(),
        }
    }

    #[test]
    fn empty_until_active_account_event_arrives() {
        let proj = projection_for(Some(ALICE));
        assert_eq!(proj.snapshot(), SearchRelayListSnapshot::default());

        proj.on_kernel_event(&event(
            BOB,
            100,
            KIND_SEARCH_RELAYS,
            vec![vec!["relay", "wss://search.example"]],
        ));
        assert_eq!(proj.snapshot(), SearchRelayListSnapshot::default());
    }

    #[test]
    fn parses_relay_tags_dedupes_and_sorts() {
        let proj = projection_for(Some(ALICE));
        proj.on_kernel_event(&event(
            ALICE,
            100,
            KIND_SEARCH_RELAYS,
            vec![
                vec!["relay", "wss://z.example"],
                vec!["relay", "wss://a.example"],
                vec!["relay", "wss://a.example"],
                vec!["r", "wss://ignored.example"],
                vec!["relay", "https://not-a-relay.example"],
            ],
        ));

        assert_eq!(
            proj.snapshot().relays,
            vec!["wss://a.example".to_string(), "wss://z.example".to_string()]
        );
    }

    #[test]
    fn newer_event_replaces_prior_relays() {
        let proj = projection_for(Some(ALICE));
        proj.on_kernel_event(&event(
            ALICE,
            100,
            KIND_SEARCH_RELAYS,
            vec![vec!["relay", "wss://old.example"]],
        ));
        proj.on_kernel_event(&event(
            ALICE,
            101,
            KIND_SEARCH_RELAYS,
            vec![vec!["relay", "wss://new.example"]],
        ));

        assert_eq!(
            proj.snapshot().relays,
            vec!["wss://new.example".to_string()]
        );
    }

    #[test]
    fn older_event_does_not_replace_newer_relays() {
        let proj = projection_for(Some(ALICE));
        proj.on_kernel_event(&event(
            ALICE,
            101,
            KIND_SEARCH_RELAYS,
            vec![vec!["relay", "wss://new.example"]],
        ));
        proj.on_kernel_event(&event(
            ALICE,
            100,
            KIND_SEARCH_RELAYS,
            vec![vec!["relay", "wss://old.example"]],
        ));

        assert_eq!(
            proj.snapshot().relays,
            vec!["wss://new.example".to_string()]
        );
    }

    #[test]
    fn account_switch_hides_stale_relays() {
        let slot = Arc::new(Mutex::new(Some(ALICE.to_string())));
        let proj = SearchRelayListProjection::new(Arc::clone(&slot));
        proj.on_kernel_event(&event(
            ALICE,
            100,
            KIND_SEARCH_RELAYS,
            vec![vec!["relay", "wss://alice.example"]],
        ));
        assert!(!proj.snapshot().relays.is_empty());

        *slot.lock().expect("slot") = Some(BOB.to_string());
        assert_eq!(proj.snapshot(), SearchRelayListSnapshot::default());
    }
}
