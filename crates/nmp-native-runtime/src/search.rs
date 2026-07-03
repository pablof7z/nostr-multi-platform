//! NIP-50 host capability wiring for the native runtime.
//!
//! `nmp-native-runtime` implements the narrow [`nmp_nip50::SearchHost`] seam so
//! the concept-owned `nmp_nip50::open_search` doorway can drive native hosts.
//! The runtime stores platform resources; it does not define NIP-50 session
//! doorways.

use std::sync::Arc;

use nmp_core::substrate::PreferredRelaySource;
use nmp_nip50::SearchHost;
use nmp_store::EventStore;

use super::NmpApp;

impl NmpApp {
    /// Store the host-installed preferred-relay source (the substrate-generic
    /// [`PreferredRelaySource`] seam). Last-writer-wins; a poisoned slot is a
    /// silent no-op (D6).
    pub fn install_preferred_relay_source(&self, source: Arc<dyn PreferredRelaySource>) {
        if let Ok(mut slot) = self.capability_ports.search_relay_source.lock() {
            *slot = Some(source);
        }
    }
}

impl SearchHost for NmpApp {
    fn search_relay_source(&self) -> Option<Arc<dyn PreferredRelaySource>> {
        self.capability_ports
            .search_relay_source
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
    }

    fn search_event_store(&self) -> Option<Arc<dyn EventStore>> {
        self.read_handles
            .event_store_handle
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
    }

    fn search_snapshot_payload(&self, projection_key: &str) -> Option<Vec<u8>> {
        self.run_typed_snapshot_projections()
            .into_iter()
            // A removed key surfaces once as a `Cleared` row with an empty
            // payload (snapshot registry drains it exactly once on unregister);
            // an empty buffer is never a valid `N50S` snapshot, so filtering it
            // out makes a closed session read as `None`.
            .find(|d| d.key == projection_key && !d.payload.is_empty())
            .map(|d| d.payload)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use nmp_nip50::{
        close_search, open_search, search_snapshot_bytes, Nip50SearchHandle, Nip50SearchSession,
        SearchRequest, SearchScope, SearchTargets,
    };

    const KIND_SHORT_TEXT_NOTE: u32 = 1;

    #[test]
    fn stale_typed_search_handle_does_not_close_replacement() {
        let app = crate::new_app();
        let first = search_request("nostr");
        let second = search_request("relay");

        let first_handle = open_search(&app, Nip50SearchSession::new(first, "native-search"));
        assert!(
            search_snapshot_bytes(&app, &first_handle).is_some(),
            "initial search sidecar should be registered"
        );

        let second_handle = open_search(&app, Nip50SearchSession::new(second, "native-search"));
        close_search(&app, &first_handle);
        assert!(
            search_snapshot_bytes(&app, &second_handle).is_some(),
            "stale typed close must not remove the replacement session"
        );

        close_search(&app, &Nip50SearchHandle::for_key("native-search"));
        assert!(
            search_snapshot_bytes(&app, &second_handle).is_none(),
            "legacy key close still removes the live session"
        );
    }

    fn search_request(query: &str) -> SearchRequest {
        SearchRequest::new(
            query,
            SearchScope::Kinds(BTreeSet::from([KIND_SHORT_TEXT_NOTE])),
            SearchTargets::Explicit(Vec::new()),
            Some(10),
        )
        .expect("valid search request")
    }
}
