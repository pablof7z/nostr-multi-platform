//! Higher-order NIP-50 search C-ABI surface + the reusable `NmpApp` Rust API.
//!
//! `nmp-ffi` is the composition root that owns the `NmpApp` actor handle, so the
//! host-driving search entrypoint lives here (the same role
//! `nmp_app_chirp_open_author_feed` plays for the author feed, but reusable by
//! every `NmpApp` host — the Rust app `hl` calls [`NmpApp::open_search`]
//! directly; the iOS app Chirp calls the C-ABI thin shell). The orchestration
//! primitives — relay resolution, the per-relay relay-pinned interest plan, the
//! deduplicating result projection, and the typed `N50S` snapshot codec — are
//! owned by `nmp-nip50` (D0: `nmp-nip50` never names `nmp-ffi`).
//!
//! ## What one open does ([`NmpApp::open_search`])
//!
//! 1. **Relay resolution** — `nmp_nip50::resolve_search_relays` resolves the
//!    request's [`SearchTargets`](nmp_nip50::SearchTargets) against the
//!    host-registered [`nmp_nip50::SearchRelaySource`] (`UserPreferred` →
//!    kind:10007; `AppDefault` → app default; `Explicit` → the given list).
//! 2. **Projection** — a [`nmp_nip50::SearchResultsProjection`] is registered
//!    as a MUTED kernel event observer (the live relay-hit ingest seam) and a
//!    typed `N50S` snapshot sidecar is registered under
//!    `nmp.nip50.search.<session>` reading its snapshot.
//! 3. **Fan-out** — `nmp_nip50::search_relay_plan` builds one relay-pinned
//!    `InterestShape` per resolved relay; each is opened via
//!    [`NmpApp::open_observed_interest_pinned`], which routes it to exactly that
//!    relay (the planner's relay-pin lane) and activates the observer. The
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
//!   their results arrive event-by-event through the muted→active kernel
//!   observer, tagged `Relay(url)`. First arrival wins on a duplicate id, so a
//!   cache hit (ingested synchronously at open) keeps its `Cache` tag when the
//!   relay later echoes the same event.
//!
//! ## D6
//!
//! Every C entry point is fire-and-forget: null pointers, invalid UTF-8,
//! malformed JSON, and poisoned mutexes degrade silently (a no-op open, an
//! empty snapshot) rather than crossing the FFI as a panic.

use std::ffi::{c_char, c_int};
use std::sync::{Arc, Mutex};

use nmp_core::__ffi_internal::register_rust_observer_muted;
use nmp_core::substrate::PreferredRelaySource;
use nmp_core::KernelEventObserverId;
use nmp_feed::DEFAULT_FEED_WINDOW_LIMIT;
use nmp_nip50::{
    encode_search_results_snapshot, search_relay_plan, SearchRequest, SearchResultsProjection,
    SearchTargets, SEARCH_RESULTS_FILE_IDENTIFIER, SEARCH_RESULTS_SCHEMA_ID,
    SEARCH_RESULTS_SCHEMA_VERSION,
};

use super::{app_ref, c_string_argument, NmpApp};

/// `1` = Global. A search interest pins a concrete relay + query; it is NOT
/// re-routed on account switch (the host closes + re-opens on identity change).
const SCOPE_GLOBAL: u32 = 1;

/// `&self` [`KernelEventObserver`](nmp_core::KernelEventObserver) adapter over
/// the `&mut self` [`SearchResultsProjection`]. Locks the shared projection on
/// each fanned-out event and ingests it as a relay hit (the delivering relay is
/// the event's first `relay_provenance` entry). A poisoned lock degrades to a
/// dropped event (D6) rather than a panic across the kernel fan-out.
struct SearchObserver(Arc<Mutex<SearchResultsProjection>>);

impl nmp_core::KernelEventObserver for SearchObserver {
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
    /// The muted→active kernel observer id (the result projection).
    observer_id: KernelEventObserverId,
    /// Per-relay `(filter_json, consumer_id, relay_pin)` close args, matching
    /// each pinned open so the kernel reconstructs the same registry slot.
    relay_closes: Vec<(String, String, String)>,
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
        if let Ok(mut slot) = self.search_relay_source.lock() {
            *slot = Some(source);
        }
    }

    /// Resolve the effective relay set for `targets` from the installed
    /// [`PreferredRelaySource`] (UserPreferred → `primary`, falling back to
    /// `fallback` when empty; AppDefault → `fallback`; Explicit → the given
    /// list). De-duplicated, first-seen order. Empty when no source installed.
    fn resolve_search_relays(&self, targets: &SearchTargets) -> Vec<String> {
        let (primary, fallback) = self
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

    /// Open a NIP-50 search session — the reusable Rust API `hl` calls directly.
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
    pub fn open_search(&self, request: SearchRequest, session_id: &str) -> String {
        // Idempotent re-open: drop any prior session under this id first.
        self.close_search(session_id);

        let relays = self.resolve_search_relays(&request.targets);

        let key = search_key(session_id);
        // #1827's `SearchResultsProjection` is `&mut self` (the cache-FTS owner);
        // wrap it in an `Arc<Mutex<…>>` so it can be BOTH a `&self`
        // KernelEventObserver (live relay-hit ingest) AND read by the typed
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
        if let Some(store) = self.event_store_handle.lock().ok().and_then(|slot| slot.clone()) {
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

        // Register the projection (via the `&self` observer adapter) as a MUTED
        // observer; each pinned open below activates it (idempotent activation)
        // so LIVE relay events fan out to it. The generic read-cache replay is
        // deliberately suppressed (empty `replay_shapes` below) — cache hits are
        // served by the search-filtered FTS scan above, never by the structural
        // replay gate that ignores the query text (#1882).
        let observer_id = register_rust_observer_muted(
            &self.event_observers,
            Arc::new(SearchObserver(Arc::clone(&projection))) as Arc<dyn nmp_core::KernelEventObserver>,
        );

        // One relay-pinned observed interest per resolved relay.
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
                },
            );
        }
        key
    }

    /// Close a NIP-50 search session: detach every per-relay pinned interest,
    /// revoke the result observer, and remove the typed sidecar. Idempotent —
    /// closing an unknown session is a harmless no-op (D6).
    pub fn close_search(&self, session_id: &str) {
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
        self.unregister_event_observer(session.observer_id);
        self.remove_snapshot_projection(&session.projection_key);
    }

    /// Read the current typed `N50S` search-results buffer for a live session,
    /// or `None` when the session is unknown / its projection emitted nothing.
    /// The same bytes the host receives in the snapshot frame's typed sidecar —
    /// exposed as a synchronous pull for hosts that poll rather than diff frames.
    #[must_use]
    pub fn search_snapshot_bytes(&self, session_id: &str) -> Option<Vec<u8>> {
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
    #[cfg(test)]
    #[must_use]
    pub(crate) fn search_session_relays(&self, session_id: &str) -> Vec<String> {
        self.search_sessions
            .lock()
            .ok()
            .and_then(|sessions| {
                sessions
                    .get(session_id)
                    .map(|s| s.relay_closes.iter().map(|(_, _, relay)| relay.clone()).collect())
            })
            .unwrap_or_default()
    }
}

// ===========================================================================
// C-ABI
// ===========================================================================

/// Open a NIP-50 search session from a JSON query payload.
///
/// `request_json` is the serde JSON of an [`nmp_nip50::SearchRequest`]:
///
/// ```json
/// {"query":"nostr","scope":"Users","targets":"UserPreferred","max_hits":50}
/// ```
///
/// (`scope` accepts `"Users"`, `"LongForm"`, `{"Kinds":[1,30023]}`;
/// `targets` accepts `"UserPreferred"`, `"AppDefault"`, `{"Explicit":["wss://…"]}`.)
/// JSON-in matches the established open-style convention (`nmp_app_open_interest`
/// takes `filter_json`); the SNAPSHOT OUTPUT is typed FlatBuffers (`N50S`),
/// registered under `nmp.nip50.search.<session_id>`.
///
/// `session_id` keys the session for `nmp_app_search_close` and the projection
/// key. A query failing NIP-50 bounded validation, malformed JSON, a null
/// pointer, or non-UTF-8 input is a silent no-op (D6).
///
/// # Safety
/// `app` must be a valid pointer from [`super::nmp_app_new`] (or null);
/// `request_json` / `session_id` must be valid NUL-terminated C strings (or null).
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_search_open(
    app: *mut NmpApp,
    request_json: *const c_char,
    session_id: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(request_json) = c_string_argument(request_json) else {
        return;
    };
    let Some(session_id) = c_string_argument(session_id).filter(|s| !s.is_empty()) else {
        return;
    };
    let Some(request) = parse_search_request(&request_json) else {
        return;
    };
    let _ = app.open_search(request, &session_id);
}

/// Close a NIP-50 search session opened via [`nmp_app_search_open`]. Idempotent;
/// a null/unknown session is a no-op (D6).
///
/// # Safety
/// `app` must be a valid pointer from [`super::nmp_app_new`] (or null);
/// `session_id` must be a valid NUL-terminated C string (or null).
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_search_close(app: *mut NmpApp, session_id: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(session_id) = c_string_argument(session_id).filter(|s| !s.is_empty()) else {
        return;
    };
    app.close_search(&session_id);
}

/// Copy the current typed `N50S` search-results buffer for a session into
/// `out_buf` (capacity `cap` bytes). Returns the number of bytes the buffer
/// occupies (the required size), or `0` when the session is unknown / has no
/// data. If the return value exceeds `cap`, nothing was copied and the host
/// should retry with a larger buffer (the standard two-call C size-probe).
///
/// The bytes are the same `N50S` payload carried in the snapshot frame's typed
/// sidecar under `nmp.nip50.search.<session_id>`; this pull is for hosts that
/// poll a single session rather than diff whole frames.
///
/// # Safety
/// `app` must be a valid pointer from [`super::nmp_app_new`] (or null);
/// `session_id` must be a valid NUL-terminated C string (or null); `out_buf`,
/// when non-null, must point to at least `cap` writable bytes.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_search_snapshot(
    app: *mut NmpApp,
    session_id: *const c_char,
    out_buf: *mut u8,
    cap: usize,
) -> c_int {
    let Some(app) = app_ref(app) else {
        return 0;
    };
    let Some(session_id) = c_string_argument(session_id).filter(|s| !s.is_empty()) else {
        return 0;
    };
    let Some(bytes) = app.search_snapshot_bytes(&session_id) else {
        return 0;
    };
    let needed = bytes.len();
    if !out_buf.is_null() && needed <= cap {
        // SAFETY: `out_buf` points to >= `cap` >= `needed` writable bytes per
        // the contract; `bytes` is a distinct owned Vec (no overlap).
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_buf, needed);
        }
    }
    c_int::try_from(needed).unwrap_or(c_int::MAX)
}

/// Parse the C-ABI JSON request payload into a validated [`SearchRequest`].
///
/// The JSON mirrors `SearchRequest`'s serde shape but re-runs `SearchRequest::new`
/// so the NIP-50 bounded-query validation + `max_hits` cap apply (a host cannot
/// bypass them by hand-crafting JSON). Returns `None` on malformed JSON or a
/// query that fails validation.
///
/// `pub(crate)` so the #1804 input-intent dispatch lane
/// ([`crate::intent_ffi`]) re-validates a `TextQuery` candidate's opaque
/// `request_json` through the same NIP-50 bounded-query constructor before
/// opening a search session — a `TextQuery` produced by `nmp_intent::classify`
/// must not bypass the cap any more than a hand-crafted `nmp_app_search_open`
/// payload can.
pub(crate) fn parse_search_request(json: &str) -> Option<SearchRequest> {
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

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
