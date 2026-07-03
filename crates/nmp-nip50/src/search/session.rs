//! `open_search_read` — the concept-owned NIP-50 active read (#2777).
//!
//! This is the NIP-50 owner's lifecycle door. It resolves relay targets,
//! seeds the result projection from the store's FTS index, compiles live
//! relay-pinned demand, and drives everything through `nmp-read-session`.
//! It contains NO registry, close map, replay implementation, or teardown
//! recipe of its own; those mechanics belong to the shared read engine.

use std::sync::{Arc, Mutex};

use nmp_core::substrate::KernelEvent;
use nmp_core::{ObservedProjectionSink, TypedProjectionData};
use nmp_ownership::FrameworkProjectionKey;
use nmp_read_session::{
    close_read, open_read, ReadDemand, ReadHandle, ReadHost, ReadOutputEncoder, ReadReplayPolicy,
    ReadSpec,
};
use nmp_store::EventStore;

use crate::{
    encode_search_results_snapshot, resolve_search_relays, search_relay_plan, SearchRelaySource,
    SearchRequest, SearchResultsProjection, SEARCH_RESULTS_FILE_IDENTIFIER,
    SEARCH_RESULTS_SCHEMA_ID, SEARCH_RESULTS_SCHEMA_VERSION,
};

/// `1` = Global. Search interests pin concrete relays and are not re-routed on
/// account switch; callers close/reopen to change identity.
const SEARCH_READ_SCOPE_GLOBAL: u32 = 1;

/// Runtime close/read handle for one NIP-50 search read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchReadHandle(ReadHandle);

impl SearchReadHandle {
    #[must_use]
    pub fn projection_key(&self) -> &str {
        &self.0.projection_key
    }
}

/// Result of opening a search read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenSearchRead {
    pub handle: SearchReadHandle,
    /// Resolved live relay pins. Empty means cache-only search.
    pub relays: Vec<String>,
}

struct SearchObserver(Arc<Mutex<SearchResultsProjection>>);

impl ObservedProjectionSink for SearchObserver {
    fn on_kernel_event(&self, event: &KernelEvent) {
        let relay = event.relay_provenance.first().cloned().unwrap_or_default();
        if let Ok(mut projection) = self.0.lock() {
            projection.ingest_relay_event(event, relay);
        }
    }
}

/// The snapshot-projection key for a search session.
#[must_use]
pub fn search_projection_key(session_id: &str) -> String {
    format!("nmp.nip50.search.{session_id}")
}

/// Refcount-owner key for one relay's search interest within a session.
#[must_use]
pub fn search_consumer(session_id: &str, relay: &str) -> String {
    format!("search-{session_id}-{relay}")
}

/// Open a NIP-50 search read through the shared read-session engine.
#[must_use]
pub fn open_search_read(
    host: &dyn ReadHost,
    request: SearchRequest,
    session_id: &str,
    relay_source: Option<&dyn SearchRelaySource>,
    store: Option<&dyn EventStore>,
) -> OpenSearchRead {
    let _ = close_search_read_by_key(host, session_id);

    let relays = relay_source
        .map(|source| resolve_search_relays(&request.targets, source))
        .unwrap_or_default();
    let key = search_projection_key(session_id);
    let projection = Arc::new(Mutex::new(SearchResultsProjection::new(request.clone())));

    if let Some(store) = store {
        if let Ok(mut projection) = projection.lock() {
            let _ = projection.ingest_cache_from_store(store);
        }
    }

    let demands = search_relay_plan(&request, &relays)
        .into_iter()
        .map(|pinned| ReadDemand {
            filter_json: nmp_core::subs::filter_json_for(&pinned.shape),
            consumer_id: search_consumer(session_id, &pinned.relay),
            scope: SEARCH_READ_SCOPE_GLOBAL,
            relay_pin: Some(pinned.relay),
            replay_limit: 0,
            replay: ReadReplayPolicy::LiveOnly,
        })
        .collect();

    let projection_for_output = Arc::clone(&projection);
    let output_key = key.clone();
    let output_encoder: ReadOutputEncoder = Box::new(move || {
        let snapshot = projection_for_output.lock().ok()?.snapshot();
        Some(TypedProjectionData {
            key: output_key.clone(),
            schema_id: SEARCH_RESULTS_SCHEMA_ID.to_string(),
            schema_version: SEARCH_RESULTS_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(SEARCH_RESULTS_FILE_IDENTIFIER).into_owned(),
            payload: encode_search_results_snapshot(&snapshot),
            ..Default::default()
        })
    });

    let projection_key = FrameworkProjectionKey::declared(key, "projection.nmp.nip50.search")
        .expect("search projection keys use the nmp.nip50.search family");
    let handle = open_read(
        host,
        ReadSpec {
            projection_key: projection_key.into(),
            demands,
            observer: Arc::new(SearchObserver(projection)) as Arc<dyn ObservedProjectionSink>,
            output_encoder,
            dependent_demands: Vec::new(),
            keep_open_without_live_demand: true,
        },
    );

    OpenSearchRead {
        handle: SearchReadHandle(handle),
        relays,
    }
}

/// Close a search read by its engine-owned handle.
#[must_use]
pub fn close_search_read(host: &dyn ReadHost, handle: &SearchReadHandle) -> bool {
    close_read(host, &handle.0)
}

/// Close a search read by its stable session key through the shared registry.
#[must_use]
pub fn close_search_read_by_key(host: &dyn ReadHost, session_id: &str) -> bool {
    host.close_read_session_by_projection_key(&search_projection_key(session_id))
}
