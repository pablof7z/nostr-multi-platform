//! Browser-runtime NIP-50 search sessions.
//!
//! This mirrors the native `NmpApp::open_search` composition role without
//! depending on `nmp-ffi`: `nmp-nip50` owns request validation, relay-pinned
//! fanout, cache ingestion, result projection, and the typed `N50S` codec.

use nmp_nip50::{
    close_search_read, close_search_read_by_key, open_search_read, SearchReadHandle,
    SearchRelaySource, SearchRequest,
};

use super::handle::BrowserRuntimeHandle;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserSearchSessionDescriptor {
    pub(crate) request: SearchRequest,
    pub(crate) key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserSearchSessionHandle {
    key: String,
    read_handle: Option<SearchReadHandle>,
}

impl BrowserSearchSessionHandle {
    #[must_use]
    pub(crate) fn for_key(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            read_handle: None,
        }
    }
}

struct PreferredRelaySearchSource<'a>(&'a dyn nmp_core::substrate::PreferredRelaySource);

impl SearchRelaySource for PreferredRelaySearchSource<'_> {
    fn user_preferred(&self) -> Vec<String> {
        self.0.primary()
    }

    fn app_default(&self) -> Vec<String> {
        self.0.fallback()
    }
}
impl BrowserRuntimeHandle {
    pub(crate) fn open_search_session(
        &mut self,
        descriptor: BrowserSearchSessionDescriptor,
    ) -> BrowserSearchSessionHandle {
        let opened = self.open_search_for_key(descriptor.request, &descriptor.key);
        BrowserSearchSessionHandle {
            key: descriptor.key,
            read_handle: Some(opened.handle),
        }
    }

    pub(crate) fn close_search_session(&mut self, handle: BrowserSearchSessionHandle) {
        if let Some(read_handle) = handle.read_handle.as_ref() {
            let _ = close_search_read(self, read_handle);
            return;
        }
        let _ = close_search_read_by_key(self, &handle.key);
    }

    pub(crate) fn open_search_for_key(
        &mut self,
        request: SearchRequest,
        session_id: &str,
    ) -> nmp_nip50::search::OpenSearchRead {
        let store = self.runtime.reducer.event_store_handle();
        let source = self
            .preferred_relay_source
            .as_deref()
            .map(PreferredRelaySearchSource);
        let source = source
            .as_ref()
            .map(|source| source as &dyn SearchRelaySource);
        open_search_read(self, request, session_id, source, Some(store.as_ref()))
    }
}
