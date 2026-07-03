//! NIP-50 host capability wiring for the browser runtime.
//!
//! `nmp-browser-runtime` implements the narrow [`nmp_nip50::SearchHost`] seam
//! so the concept-owned `nmp_nip50::open_search` doorway can drive browser
//! hosts. The runtime stores platform resources; it does not define NIP-50
//! session doorways.

use std::sync::Arc;

use nmp_core::substrate::PreferredRelaySource;
use nmp_nip50::SearchHost;
use nmp_store::EventStore;

use super::handle::BrowserRuntimeHandle;

impl SearchHost for BrowserRuntimeHandle {
    fn search_relay_source(&self) -> Option<Arc<dyn PreferredRelaySource>> {
        self.preferred_relay_source.clone()
    }

    fn search_event_store(&self) -> Option<Arc<dyn EventStore>> {
        Some(self.runtime.reducer.event_store_handle())
    }
}
