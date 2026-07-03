//! `open_search_read` — the concept-owned NIP-50 active read (#2777).
//!
//! This is the NIP-50 owner's lifecycle door. It resolves relay targets,
//! seeds the result projection from the store's FTS index, compiles live
//! relay-pinned demand, and drives everything through `nmp-read-session`.
//! It contains NO registry, close map, replay implementation, or teardown
//! recipe of its own; those mechanics belong to the shared read engine.

use std::sync::{Arc, Mutex};

use nmp_core::substrate::{KernelEvent, PreferredRelaySource};
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

/// Host capabilities the NIP-50 public search doorway needs.
///
/// Runtime crates implement this once beside their generic [`ReadHost`] impl.
/// The concept crate owns the public `open_search` / `close_search` lifecycle;
/// the host only supplies runtime-owned resources without growing NIP-50
/// methods of its own.
pub trait SearchHost: ReadHost {
    /// The host-installed preferred relay source used for `UserPreferred` and
    /// `AppDefault` target resolution.
    fn search_relay_source(&self) -> Option<Arc<dyn PreferredRelaySource>>;

    /// The host event store used for bounded cache seeding.
    fn search_event_store(&self) -> Option<Arc<dyn EventStore>>;

    /// Pull the latest typed snapshot payload for `projection_key`, when the
    /// host exposes a synchronous read surface. Push-frame-only hosts can use
    /// the default and deliver snapshots through their normal frame pipeline.
    fn search_snapshot_payload(&self, _projection_key: &str) -> Option<Vec<u8>> {
        None
    }
}

/// Descriptor for one host-driven NIP-50 search read session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip50SearchSession {
    request: SearchRequest,
    key: String,
}

impl Nip50SearchSession {
    /// Build a search descriptor with a caller-stable key.
    #[must_use]
    pub fn new(request: SearchRequest, key: impl Into<String>) -> Self {
        Self {
            request,
            key: key.into(),
        }
    }

    #[must_use]
    pub fn request(&self) -> &SearchRequest {
        &self.request
    }
}

/// Runtime handle for one NIP-50 search read session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip50SearchHandle {
    key: String,
    projection_key: String,
    read_handle: Option<SearchReadHandle>,
}

impl Nip50SearchHandle {
    /// Reconstruct the typed handle used by legacy close/read operations.
    #[must_use]
    pub fn for_key(key: impl Into<String>) -> Self {
        let key = key.into();
        Self {
            projection_key: search_projection_key(&key),
            key,
            read_handle: None,
        }
    }

    #[must_use]
    pub fn projection_key(&self) -> &str {
        &self.projection_key
    }
}

struct PreferredRelaySearchSource<'a>(&'a dyn PreferredRelaySource);

impl SearchRelaySource for PreferredRelaySearchSource<'_> {
    fn user_preferred(&self) -> Vec<String> {
        self.0.primary()
    }

    fn app_default(&self) -> Vec<String> {
        self.0.fallback()
    }
}

/// Open a NIP-50 search session through the concept-owned lifecycle doorway.
#[must_use]
pub fn open_search<H>(host: &H, descriptor: Nip50SearchSession) -> Nip50SearchHandle
where
    H: SearchHost,
{
    let opened = open_search_for_key(host, descriptor.request, &descriptor.key);
    Nip50SearchHandle {
        key: descriptor.key,
        projection_key: opened.handle.projection_key().to_string(),
        read_handle: Some(opened.handle),
    }
}

/// Close a NIP-50 search session by its typed handle.
pub fn close_search<H>(host: &H, handle: &Nip50SearchHandle) -> bool
where
    H: ReadHost,
{
    if let Some(read_handle) = handle.read_handle.as_ref() {
        return close_search_read(host, read_handle);
    }
    close_search_read_by_key(host, &handle.key)
}

/// Read the current typed `N50S` search-results buffer for a live session.
#[must_use]
pub fn search_snapshot_bytes<H>(host: &H, handle: &Nip50SearchHandle) -> Option<Vec<u8>>
where
    H: SearchHost,
{
    host.search_snapshot_payload(handle.projection_key())
}

fn open_search_for_key<H>(host: &H, request: SearchRequest, session_id: &str) -> OpenSearchRead
where
    H: SearchHost,
{
    let relay_source = host.search_relay_source();
    let relay_source = relay_source.as_deref().map(PreferredRelaySearchSource);
    let relay_source = relay_source
        .as_ref()
        .map(|source| source as &dyn SearchRelaySource);
    let store = host.search_event_store();
    open_search_read(host, request, session_id, relay_source, store.as_deref())
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

/// Parse the JSON search request payload into a validated [`SearchRequest`].
///
/// The JSON mirrors `SearchRequest`'s serde shape but re-runs
/// [`SearchRequest::new`] so the NIP-50 bounded-query validation and
/// `max_hits` cap apply.
pub fn parse_search_request(json: &str) -> Option<SearchRequest> {
    #[derive(serde::Deserialize)]
    struct Dto {
        query: String,
        scope: crate::SearchScope,
        targets: crate::SearchTargets,
        #[serde(default)]
        max_hits: Option<usize>,
    }
    let dto: Dto = serde_json::from_str(json).ok()?;
    SearchRequest::new(&dto.query, dto.scope, dto.targets, dto.max_hits)
}
