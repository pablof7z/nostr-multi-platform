//! Higher-order NIP-50 search runtime orchestration + reusable `NmpApp` Rust API.
//!
//! `nmp-native-runtime` is the composition root that owns the `NmpApp` actor
//! handle, so the host-driving search entrypoint lives here (the same
//! composition role `NmpApp::open_feed` plays for declared feeds, but reusable
//! by every `NmpApp` host; C ABI callers reach it through the thin `nmp-ffi`
//! wrapper). The orchestration
//! primitives — relay resolution, the per-relay relay-pinned interest plan, the
//! deduplicating result projection, and the typed `N50S` snapshot codec — are
//! owned by `nmp-nip50` (D0: `nmp-nip50` never names the native runtime or
//! `nmp-ffi`).
//!
//! ## What one open does ([`NmpApp::open_search`])
//!
//! 1. **Relay resolution** — `nmp_nip50::resolve_search_relays` resolves the
//!    request's [`SearchTargets`](nmp_nip50::SearchTargets) against the
//!    host-registered [`nmp_nip50::SearchRelaySource`] (`UserPreferred` →
//!    kind:10007; `AppDefault` → app default; `Explicit` → the given list).
//! 2. **Projection** — a [`nmp_nip50::SearchResultsProjection`] is registered
//!    as a muted observed-projection sink (the live relay-hit ingest seam) and a
//!    typed `N50S` snapshot sidecar is registered under
//!    `nmp.nip50.search.<session>` reading its snapshot.
//! 3. **Fan-out** — `nmp_nip50::search_relay_plan` builds one relay-pinned
//!    `InterestShape` per resolved relay; each is opened via
//!    [`NmpApp::open_observed_interest_pinned`], which routes it to exactly that
//!    relay (the planner's relay-pin lane) and activates the sink. The
//!    router's blocked-relay subtractive post-pass still applies, so a
//!    pinned-but-blocked relay is dropped by the same generic mechanism that
//!    guards every interest. This lane is the LIVE relay-hit seam only — it
//!    opens with NO read-cache replay (empty `replay_shapes`).
//!
//! Cache and live relay hits arrive through two distinct seams:
//!
//! - **Cache** — at open time `open_search` runs one bounded
//!   [`nmp_nip50::SearchResultsProjection::ingest_cache_from_store`] over the
//!   request's scope, a search-TEXT-filtered scan through the store's NIP-50
//!   full-text index (`EventStore::text_search_visit`, #1827). Matches are
//!   tagged [`nmp_nip50::SearchHitSource::Cache`]. A cached event that does NOT
//!   match the query text is never returned (#1882). This deliberately does NOT
//!   go through the generic observed-interest replay, whose structural gate
//!   (`InterestShape::matches_event_with_id`) filters by kind + time only and
//!   would surface unrelated cached events mislabelled `Relay("")`.
//! - **Live relay** — the per-relay pinned interests fan a NIP-50 `REQ` out and
//!   their results arrive event-by-event through the muted-to-scoped observed
//!   projection sink, tagged `Relay(url)`. First arrival wins on a duplicate id, so a
//!   cache hit (ingested synchronously at open) keeps its `Cache` tag when the
//!   relay later echoes the same event.
//!
//! ## D6
//!
//! Every C entry point is fire-and-forget: null pointers, invalid UTF-8,
//! malformed JSON, and poisoned mutexes degrade silently (a no-op open, an
//! empty snapshot) rather than crossing the FFI as a panic.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nmp_core::__ffi_internal::register_rust_observer_muted;
use nmp_core::ObservedProjectionId;
use nmp_core::substrate::PreferredRelaySource;
use nmp_feed::DEFAULT_FEED_WINDOW_LIMIT;
use nmp_nip50::{
    SEARCH_RESULTS_FILE_IDENTIFIER, SEARCH_RESULTS_SCHEMA_ID, SEARCH_RESULTS_SCHEMA_VERSION,
    SearchRequest, SearchResultsProjection, SearchTargets, encode_search_results_snapshot,
    search_relay_plan,
};

use super::NmpApp;

/// `1` = Global. A search interest pins a concrete relay + query; it is NOT
/// re-routed on account switch (the host closes + re-opens on identity change).
const SCOPE_GLOBAL: u32 = 1;
static NEXT_SEARCH_HANDLE_ID: AtomicU64 = AtomicU64::new(1);

/// Descriptor for one host-driven NIP-50 search read session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip50SearchSession {
    request: SearchRequest,
    key: String,
}

impl Nip50SearchSession {
    /// Build a search descriptor with a caller-stable key.
    ///
    /// FFI adapters use the key to preserve their wire contract; Rust callers
    /// pass the returned [`Nip50SearchHandle`] back for close/read operations.
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
    handle_id: u64,
}

impl Nip50SearchHandle {
    /// Reconstruct the typed handle used by legacy FFI close/read operations.
    #[must_use]
    pub fn for_key(key: impl Into<String>) -> Self {
        let key = key.into();
        Self {
            projection_key: search_key(&key),
            key,
            handle_id: 0,
        }
    }

    #[must_use]
    pub fn projection_key(&self) -> &str {
        &self.projection_key
    }
}

/// `&self` [`ObservedProjectionSink`](nmp_core::ObservedProjectionSink) adapter over
/// the `&mut self` [`SearchResultsProjection`]. Locks the shared projection on
/// each fanned-out event and ingests it as a relay hit (the delivering relay is
/// the event's first `relay_provenance` entry). A poisoned lock degrades to a
/// dropped event (D6) rather than a panic across the kernel fan-out.
struct SearchObserver(Arc<Mutex<SearchResultsProjection>>);

impl nmp_core::ObservedProjectionSink for SearchObserver {
    fn on_kernel_event(&self, event: &nmp_core::substrate::KernelEvent) {
        let relay = event.relay_provenance.first().cloned().unwrap_or_default();
        if let Ok(mut projection) = self.0.lock() {
            projection.ingest_relay_event(event, relay);
        }
    }
}

/// Teardown recipe for one live search session (held in
/// `NmpApp::search_sessions`). Records exactly what
/// [`NmpApp::open_search`] installed so [`NmpApp::close_search`] reverses it.
pub(crate) struct SearchSession {
    /// `nmp.nip50.search.<session_id>` — the typed sidecar key.
    projection_key: String,
    /// The single muted→active kernel observer id (the shared result
    /// projection). ONE observer backs every relay's pinned interest, so the
    /// global fan-out processes each accepted event exactly once regardless of
    /// how many relays the session targets.
    observer_id: ObservedProjectionId,
    /// Per-relay `(filter_json, consumer_id, relay_pin)` close args, matching
    /// each pinned open so the kernel reconstructs the same registry slot.
    relay_closes: Vec<(String, String, String)>,
    handle_id: u64,
}

/// The snapshot-projection key for a search session.
#[must_use]
fn search_key(session_id: &str) -> String {
    format!("nmp.nip50.search.{session_id}")
}

/// Refcount-owner key for one relay's search interest within a session.
#[must_use]
fn search_consumer(session_id: &str, relay: &str) -> String {
    format!("search-{session_id}-{relay}")
}

impl NmpApp {
    /// Store the host-installed preferred-relay source (the substrate-generic
    /// [`PreferredRelaySource`] seam — NIP-50's kind:10007 read + app-default
    /// fallback). The `HostCapabilities::install_preferred_relay_source` override
    /// forwards here; `open_search` reads it back. Last-writer-wins; a poisoned
    /// slot is a silent no-op (D6).
    pub fn install_preferred_relay_source(&self, source: Arc<dyn PreferredRelaySource>) {
        if let Ok(mut slot) = self.capability_ports.search_relay_source.lock() {
            *slot = Some(source);
        }
    }

    /// Resolve the effective relay set for `targets` from the installed
    /// [`PreferredRelaySource`] (UserPreferred → `primary`, falling back to
    /// `fallback` when empty; AppDefault → `fallback`; Explicit → the given
    /// list). De-duplicated, first-seen order. Empty when no source installed.
    fn resolve_search_relays(&self, targets: &SearchTargets) -> Vec<String> {
        let (primary, fallback) = self
            .capability_ports
            .search_relay_source
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|s| (s.primary(), s.fallback())))
            .unwrap_or_default();
        let raw = match targets {
            SearchTargets::Explicit(list) => list.clone(),
            SearchTargets::UserPreferred => {
                if primary.is_empty() {
                    fallback
                } else {
                    primary
                }
            }
            SearchTargets::AppDefault => fallback,
        };
        let mut seen = std::collections::BTreeSet::new();
        raw.into_iter()
            .filter(|r| !r.is_empty() && seen.insert(r.clone()))
            .collect()
    }

    /// Open a NIP-50 search session.
    #[must_use]
    pub fn open_search_session(&self, descriptor: Nip50SearchSession) -> Nip50SearchHandle {
        let handle_id = NEXT_SEARCH_HANDLE_ID.fetch_add(1, Ordering::Relaxed);
        let projection_key =
            self.open_search_for_key(descriptor.request, &descriptor.key, handle_id);
        Nip50SearchHandle {
            key: descriptor.key,
            projection_key,
            handle_id,
        }
    }

    /// Close a NIP-50 search session by its typed handle.
    pub fn close_search_session(&self, handle: &Nip50SearchHandle) {
        if self
            .search_sessions
            .lock()
            .ok()
            .and_then(|sessions| sessions.get(&handle.key).map(|s| s.handle_id))
            .is_some_and(|open_handle_id| {
                handle.handle_id == 0 || open_handle_id == handle.handle_id
            })
        {
            self.close_search_key(&handle.key);
        }
    }

    /// Read the current typed `N50S` search-results buffer for a live session.
    #[must_use]
    pub fn search_session_snapshot_bytes(&self, handle: &Nip50SearchHandle) -> Option<Vec<u8>> {
        self.search_snapshot_bytes_for_key(&handle.key)
    }

    /// Open a NIP-50 search session.
    ///
    /// `request` is the validated [`SearchRequest`]; `session_id` keys the
    /// session for teardown and the snapshot projection. Registers the result
    /// projection + typed `N50S` sidecar under `nmp.nip50.search.<session_id>`,
    /// populates it with search-text-filtered CACHE hits from the store's NIP-50
    /// FTS index ([`SearchResultsProjection::ingest_cache_from_store`], tagged
    /// `Cache`), then resolves relays from the installed [`PreferredRelaySource`]
    /// (empty when none is installed → cache-only search) and opens one
    /// relay-pinned LIVE observed interest per resolved relay (tagged
    /// `Relay(url)`). Cache hits depend on the NIP-50 search scopes being
    /// registered (`nmp_nip50::register_search_scopes`, wired by
    /// `nmp_defaults::register_defaults`); a bare app that registers none gets an
    /// empty cache scan (`Unsupported`) and is relay-only.
    ///
    /// Re-opening the same `session_id` first tears the prior session down
    /// (idempotent at the registry level). Returns the snapshot projection key
    /// the host reads results under.
    fn open_search_for_key(
        &self,
        request: SearchRequest,
        session_id: &str,
        handle_id: u64,
    ) -> String {
        // Idempotent re-open: drop any prior session under this id first.
        self.close_search_key(session_id);

        let relays = self.resolve_search_relays(&request.targets);

        let key = search_key(session_id);
        // #1827's `SearchResultsProjection` is `&mut self` (the cache-FTS owner);
        // wrap it in an `Arc<Mutex<…>>` so it can be BOTH a `&self`
        // ObservedProjectionSink (live relay-hit ingest) AND read by the typed
        // sidecar closure, while keeping the projection's single-writer model.
        let projection = Arc::new(Mutex::new(SearchResultsProjection::new(request.clone())));

        // #1882 — CACHE hits come from the store's NIP-50 full-text index, NOT
        // the generic observed-interest replay below. The replay's structural
        // gate (`InterestShape::matches_event_with_id`) filters by kind + time
        // only — it does NOT evaluate the search text — so routing cached events
        // through it would surface every kind-matching cached event regardless
        // of whether it matches the query, mislabelled `Relay("")`. Instead we
        // run one bounded `text_search_visit` over the request's scope at open
        // time (the #1827 inverted index), which tags each genuine text match
        // `SearchHitSource::Cache`. A cached event that does NOT match the query
        // text is never ingested. The store is the kernel-owned handle published
        // into `event_store_handle` after kernel construction; `None` (pre-start
        // or poisoned) degrades to a cache-empty open (D6). Cache ingest is
        // synchronous here, so it always precedes the async relay echoes below —
        // the projection's first-arrival-wins dedupe keeps the `Cache` tag on an
        // event the relay later re-delivers.
        if let Some(store) = self
            .read_handles
            .event_store_handle
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
        {
            if let Ok(mut proj) = projection.lock() {
                let _ = proj.ingest_cache_from_store(store.as_ref());
            }
        }

        // Typed N50S sidecar reading the projection snapshot every tick.
        {
            let projection_for_sidecar = Arc::clone(&projection);
            let key_for_encode = key.clone();
            self.register_typed_snapshot_projection(key.clone(), move || {
                let snapshot = projection_for_sidecar.lock().ok()?.snapshot();
                Some(nmp_core::TypedProjectionData {
                    key: key_for_encode.clone(),
                    schema_id: SEARCH_RESULTS_SCHEMA_ID.to_string(),
                    schema_version: SEARCH_RESULTS_SCHEMA_VERSION,
                    file_identifier: String::from_utf8_lossy(SEARCH_RESULTS_FILE_IDENTIFIER)
                        .into_owned(),
                    payload: encode_search_results_snapshot(&snapshot),
                    ..Default::default()
                })
            });
        }

        // Register the projection (via the `&self` observer adapter) as a SINGLE
        // MUTED observer; each pinned open below activates the SAME id
        // (idempotent activation) so LIVE relay events fan out to it exactly
        // once. One observer backs all N relay interests — a search targeting
        // many relays still processes each accepted event a single time (the
        // global fan-out walks observer slots, not interests). The generic
        // read-cache replay is deliberately suppressed (empty `replay_shapes`
        // below) — cache hits are served by the search-filtered FTS scan above,
        // never by the structural replay gate that ignores the query text (#1882).
        let observer_id = register_rust_observer_muted(
            &self.event_observers,
            Arc::new(SearchObserver(Arc::clone(&projection)))
                as Arc<dyn nmp_core::ObservedProjectionSink>,
        );

        // One relay-pinned observed interest per resolved relay, all activating
        // the one shared `observer_id` above.
        let plan = search_relay_plan(&request, &relays);
        let mut relay_closes = Vec::with_capacity(plan.len());
        for pinned in plan {
            // `relay_pin` is a client-side-only routing hint, NEVER serialized
            // onto the wire (the relay receives only the regular filter); the
            // pin travels as the explicit `Some(relay)` argument below.
            let filter_json = nmp_core::subs::filter_json_for(&pinned.shape);
            let consumer = search_consumer(session_id, &pinned.relay);
            // #1882 — open the LIVE relay subscription + activate the observer,
            // but pass NO `replay_shapes`: the kernel skips the read-cache replay
            // entirely (`replay_read_cache_to_observer` is a no-op on an empty
            // shape set), so unfiltered cached events never reach the search
            // projection. Cache is served above by the FTS scan; this lane is
            // purely the live relay-hit seam (tagged `Relay(url)` by the observer).
            self.open_observed_interest_pinned(
                &filter_json,
                &consumer,
                SCOPE_GLOBAL,
                Some(pinned.relay.clone()),
                observer_id,
                Vec::new(),
                DEFAULT_FEED_WINDOW_LIMIT,
            );
            relay_closes.push((filter_json, consumer, pinned.relay));
        }

        if let Ok(mut sessions) = self.search_sessions.lock() {
            sessions.insert(
                session_id.to_string(),
                SearchSession {
                    projection_key: key.clone(),
                    observer_id,
                    relay_closes,
                    handle_id,
                },
            );
        }
        key
    }

    /// Close a NIP-50 search session: detach every per-relay pinned interest,
    /// revoke the single result observer, and remove the typed sidecar.
    /// Idempotent — closing an unknown session is a harmless no-op (D6).
    fn close_search_key(&self, session_id: &str) {
        let session = self
            .search_sessions
            .lock()
            .ok()
            .and_then(|mut sessions| sessions.remove(session_id));
        let Some(session) = session else {
            return;
        };
        for (filter_json, consumer, relay) in &session.relay_closes {
            self.close_interest_pinned(filter_json, consumer, SCOPE_GLOBAL, Some(relay.clone()));
        }
        self.revoke_observed_projection_sink(session.observer_id);
        self.remove_snapshot_projection(&session.projection_key);
    }

    /// Read the current typed `N50S` search-results buffer for a live session,
    /// or `None` when the session is unknown / its projection emitted nothing.
    /// The same bytes the host receives in the snapshot frame's typed sidecar —
    /// exposed as a synchronous pull for hosts that poll rather than diff frames.
    #[must_use]
    fn search_snapshot_bytes_for_key(&self, session_id: &str) -> Option<Vec<u8>> {
        let key = search_key(session_id);
        self.run_typed_snapshot_projections()
            .into_iter()
            // A removed key surfaces once as a `Cleared` row with an empty
            // payload (snapshot registry drains it exactly once on
            // unregister); an empty buffer is never a valid `N50S` snapshot, so
            // filtering it out makes a closed session read as `None`.
            .find(|d| d.key == key && !d.payload.is_empty())
            .map(|d| d.payload)
    }

    /// Test-only: the resolved relay set a live search session fanned out to
    /// (the per-relay pinned interests it opened). Proves UserPreferred
    /// resolution end-to-end — that the installed `PreferredRelaySource` drove
    /// the actual search fan-out, not just `effective_search_relays`.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn search_session_relays(&self, session_id: &str) -> Vec<String> {
        self.search_sessions
            .lock()
            .ok()
            .and_then(|sessions| {
                sessions.get(session_id).map(|s| {
                    s.relay_closes
                        .iter()
                        .map(|(_, _, relay)| relay.clone())
                        .collect()
                })
            })
            .unwrap_or_default()
    }
}

/// Parse the C-ABI JSON request payload into a validated [`SearchRequest`].
///
/// The JSON mirrors `SearchRequest`'s serde shape but re-runs `SearchRequest::new`
/// so the NIP-50 bounded-query validation + `max_hits` cap apply (a host cannot
/// bypass them by hand-crafting JSON). Returns `None` on malformed JSON or a
/// query that fails validation.
///
/// Public so the #1804 input-intent dispatch lane and C ABI wrappers can
/// re-validate a `TextQuery` candidate's opaque
/// `request_json` through the same NIP-50 bounded-query constructor before
/// opening a search session — a `TextQuery` produced by `nmp_intent::classify`
/// must not bypass the cap any more than a hand-crafted `nmp_app_search_open`
/// payload can.
pub fn parse_search_request(json: &str) -> Option<SearchRequest> {
    #[derive(serde::Deserialize)]
    struct Dto {
        query: String,
        scope: nmp_nip50::SearchScope,
        targets: nmp_nip50::SearchTargets,
        #[serde(default)]
        max_hits: Option<usize>,
    }
    let dto: Dto = serde_json::from_str(json).ok()?;
    SearchRequest::new(&dto.query, dto.scope, dto.targets, dto.max_hits)
}
