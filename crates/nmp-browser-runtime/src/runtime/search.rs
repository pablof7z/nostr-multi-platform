//! Browser-runtime NIP-50 search sessions.
//!
//! This mirrors the native `NmpApp::open_search` composition role without
//! depending on `nmp-ffi`: `nmp-nip50` owns request validation, relay-pinned
//! fanout, cache ingestion, result projection, and the typed `N50S` codec.

use std::sync::{Arc, Mutex};

use nmp_core::substrate::{KernelEvent, ObservedProjection};
use nmp_core::{ObservedProjectionSink, TypedProjectionData};
use nmp_feed::DEFAULT_FEED_WINDOW_LIMIT;
use nmp_nip50::{
    encode_search_results_snapshot, resolve_search_relays, search_relay_plan, SearchRelaySource,
    SearchRequest, SearchResultsProjection, SearchSessionBuild, SearchTeardownAction,
    SEARCH_RESULTS_FILE_IDENTIFIER, SEARCH_RESULTS_SCHEMA_ID, SEARCH_RESULTS_SCHEMA_VERSION,
};

use super::handle::BrowserRuntimeHandle;

const SCOPE_GLOBAL: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserSearchSessionDescriptor {
    pub(crate) request: SearchRequest,
    pub(crate) key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserSearchSessionHandle {
    key: String,
}

impl BrowserSearchSessionHandle {
    #[must_use]
    pub(crate) fn for_key(key: impl Into<String>) -> Self {
        Self { key: key.into() }
    }
}

struct SearchObserver(Arc<Mutex<SearchResultsProjection>>);

struct PreferredRelaySearchSource<'a>(&'a dyn nmp_core::substrate::PreferredRelaySource);

impl SearchRelaySource for PreferredRelaySearchSource<'_> {
    fn user_preferred(&self) -> Vec<String> {
        self.0.primary()
    }

    fn app_default(&self) -> Vec<String> {
        self.0.fallback()
    }
}

impl ObservedProjectionSink for SearchObserver {
    fn on_kernel_event(&self, event: &KernelEvent) {
        let relay = event.relay_provenance.first().cloned().unwrap_or_default();
        if let Ok(mut projection) = self.0.lock() {
            projection.ingest_relay_event(event, relay);
        }
    }
}

impl BrowserRuntimeHandle {
    pub(crate) fn open_search_session(
        &mut self,
        descriptor: BrowserSearchSessionDescriptor,
    ) -> BrowserSearchSessionHandle {
        self.open_search_for_key(descriptor.request, &descriptor.key);
        BrowserSearchSessionHandle {
            key: descriptor.key,
        }
    }

    pub(crate) fn close_search_session(&mut self, handle: BrowserSearchSessionHandle) {
        self.close_search_key(&handle.key);
    }

    pub(crate) fn open_search_for_key(
        &mut self,
        request: SearchRequest,
        session_id: &str,
    ) -> String {
        self.close_search_key(session_id);

        let relays = self.resolve_search_relays(&request.targets);
        let projection = Arc::new(Mutex::new(SearchResultsProjection::new(request.clone())));

        let store = self.runtime.reducer.event_store_handle();
        if let Ok(mut proj) = projection.lock() {
            let _ = proj.ingest_cache_from_store(store.as_ref());
        }

        let key = search_key(session_id);
        self.register_search_sidecar(&key, Arc::clone(&projection));

        let mut teardown: Vec<SearchTeardownAction> = vec![self
            .runtime
            .reducer
            .remove_snapshot_projection_action(key.clone())];
        for pinned in search_relay_plan(&request, &relays) {
            let observer = Arc::new(SearchObserver(Arc::clone(&projection)));
            let decl = ObservedProjection {
                observer,
                filter_json: nmp_core::subs::filter_json_for(&pinned.shape),
                consumer_id: search_consumer(session_id, &pinned.relay),
                scope: SCOPE_GLOBAL,
                relay_pin: Some(pinned.relay.clone()),
                // NIP-50 cache is served above by FTS query, not by generic
                // structural cache replay. `open_live_only` clears these before
                // sending the kernel command while preserving scoped live fanout.
                replay_shapes: vec![pinned.shape],
                replay_limit: DEFAULT_FEED_WINDOW_LIMIT,
            };
            let id = self.observed_projection_registrar.open_live_only(decl);
            if id.0 != 0 {
                let registrar = self.observed_projection_registrar.clone();
                teardown.push(Box::new(move || {
                    registrar.close(id);
                }));
            }
        }

        self.search_sessions.open(
            session_id.to_string(),
            SearchSessionBuild {
                projection_key: key.clone(),
                relays,
                teardown,
            },
        );
        key
    }

    pub(crate) fn close_search_key(&mut self, session_id: &str) {
        self.search_sessions.close(session_id);
    }

    fn register_search_sidecar(
        &mut self,
        key: &str,
        projection: Arc<Mutex<SearchResultsProjection>>,
    ) {
        let key_for_row = key.to_string();
        let Ok(projection_key) = nmp_ownership::FrameworkProjectionKey::declared(
            key_for_row.clone(),
            "projection.nmp.nip50.search",
        ) else {
            return;
        };
        self.runtime
            .reducer
            .register_typed_snapshot_projection(projection_key, move || {
                let snapshot = projection.lock().ok()?.snapshot();
                Some(TypedProjectionData {
                    key: key_for_row.clone(),
                    schema_id: SEARCH_RESULTS_SCHEMA_ID.to_string(),
                    schema_version: SEARCH_RESULTS_SCHEMA_VERSION,
                    file_identifier: String::from_utf8_lossy(SEARCH_RESULTS_FILE_IDENTIFIER)
                        .into_owned(),
                    payload: encode_search_results_snapshot(&snapshot),
                    ..Default::default()
                })
            });
    }

    fn resolve_search_relays(&self, targets: &nmp_nip50::SearchTargets) -> Vec<String> {
        let Some(source) = self.preferred_relay_source.as_deref() else {
            return resolve_search_relays(targets, &(Vec::<String>::new, Vec::<String>::new));
        };
        resolve_search_relays(targets, &PreferredRelaySearchSource(source))
    }
}

fn search_key(session_id: &str) -> String {
    format!("nmp.nip50.search.{session_id}")
}

fn search_consumer(session_id: &str, relay: &str) -> String {
    format!("search-{session_id}-{relay}")
}
