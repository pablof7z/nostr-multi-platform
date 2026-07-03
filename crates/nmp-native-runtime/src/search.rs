//! Higher-order NIP-50 search runtime orchestration + reusable `NmpApp` Rust API.
//!
//! `nmp-native-runtime` is the native platform runtime adapter that owns the
//! `NmpApp` actor handle, so the host-driving search entrypoint lives here (the
//! same composition role `NmpApp::open_feed` plays for declared feeds, but
//! reusable by every `NmpApp` host; native hosts reach it through the
//! `nmp-uniffi` binding surface — there is no separate `nmp-ffi` C-ABI crate).
//! The orchestration
//! primitives — relay resolution, the per-relay relay-pinned interest plan, the
//! deduplicating result projection, and the typed `N50S` snapshot codec — are
//! owned by `nmp-nip50` (D0: `nmp-nip50` never names the native runtime or any
//! binding crate).
//!
//! ## What one open does ([`NmpApp::open_search`])
//!
//! 1. **Relay resolution** — `nmp_nip50::resolve_search_relays` resolves the
//!    request's [`SearchTargets`](nmp_nip50::SearchTargets) against the
//!    host-registered [`nmp_nip50::SearchRelaySource`] (`UserPreferred` →
//!    kind:10007; `AppDefault` → app default; `Explicit` → the given list).
//! 2. **Projection** — `nmp-nip50` builds a
//!    [`nmp_nip50::SearchResultsProjection`] as the live relay-hit ingest sink
//!    and registers a typed `N50S` snapshot sidecar under
//!    `nmp.nip50.search.<session>` through the shared read-session host seam.
//! 3. **Fan-out** — `nmp_nip50::search_relay_plan` builds one relay-pinned
//!    `InterestShape` per resolved relay; `nmp-nip50` compiles those into
//!    `nmp-read-session` live-only demands, and `NmpApp` implements
//!    [`nmp_read_session::ReadHost`] by routing each demand to exactly that
//!    relay (the planner's relay-pin lane). The router's blocked-relay
//!    subtractive post-pass still applies, so a pinned-but-blocked relay is
//!    dropped by the same generic mechanism that guards every interest. This
//!    lane is the LIVE relay-hit seam only — it opens with NO read-cache replay.
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

use std::sync::Arc;

use nmp_core::substrate::PreferredRelaySource;
use nmp_nip50::{
    close_search_read, close_search_read_by_key, open_search_read, search_projection_key,
    SearchReadHandle, SearchRelaySource, SearchRequest,
};

use super::NmpApp;

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
    read_handle: Option<SearchReadHandle>,
}

impl Nip50SearchHandle {
    /// Reconstruct the typed handle used by legacy FFI close/read operations.
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

    /// Open a NIP-50 search session.
    #[must_use]
    pub fn open_search_session(&self, descriptor: Nip50SearchSession) -> Nip50SearchHandle {
        let opened = self.open_search_for_key(descriptor.request, &descriptor.key);
        Nip50SearchHandle {
            key: descriptor.key,
            projection_key: opened.handle.projection_key().to_string(),
            read_handle: Some(opened.handle),
        }
    }

    /// Close a NIP-50 search session by its typed handle.
    pub fn close_search_session(&self, handle: &Nip50SearchHandle) {
        if let Some(read_handle) = handle.read_handle.as_ref() {
            let _ = close_search_read(self, read_handle);
            return;
        }
        let _ = close_search_read_by_key(self, &handle.key);
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
    /// registered by the crate-level `nmp_nip50::register(...)` installer; a
    /// bare app that registers none gets an empty cache scan (`Unsupported`) and
    /// is relay-only.
    ///
    /// Re-opening the same `session_id` first tears the prior session down
    /// (idempotent at the registry level). Returns the snapshot projection key
    /// the host reads results under.
    fn open_search_for_key(
        &self,
        request: SearchRequest,
        session_id: &str,
    ) -> nmp_nip50::search::OpenSearchRead {
        let source = self
            .capability_ports
            .search_relay_source
            .lock()
            .ok()
            .and_then(|slot| slot.clone());
        let source = source.as_deref().map(PreferredRelaySearchSource);
        let source = source
            .as_ref()
            .map(|source| source as &dyn SearchRelaySource);
        let store = self
            .read_handles
            .event_store_handle
            .lock()
            .ok()
            .and_then(|slot| slot.clone());
        open_search_read(self, request, session_id, source, store.as_deref())
    }

    /// Read the current typed `N50S` search-results buffer for a live session,
    /// or `None` when the session is unknown / its projection emitted nothing.
    /// The same bytes the host receives in the snapshot frame's typed sidecar —
    /// exposed as a synchronous pull for hosts that poll rather than diff frames.
    #[must_use]
    fn search_snapshot_bytes_for_key(&self, session_id: &str) -> Option<Vec<u8>> {
        let key = search_projection_key(session_id);
        self.run_typed_snapshot_projections()
            .into_iter()
            // A removed key surfaces once as a `Cleared` row with an empty
            // payload (snapshot registry drains it exactly once on
            // unregister); an empty buffer is never a valid `N50S` snapshot, so
            // filtering it out makes a closed session read as `None`.
            .find(|d| d.key == key && !d.payload.is_empty())
            .map(|d| d.payload)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use nmp_nip50::{SearchRequest, SearchScope, SearchTargets};

    use super::{Nip50SearchHandle, Nip50SearchSession};

    const KIND_SHORT_TEXT_NOTE: u32 = 1;

    #[test]
    fn stale_typed_search_handle_does_not_close_replacement() {
        let app = crate::new_app();
        let first = search_request("nostr");
        let second = search_request("relay");

        let first_handle =
            app.open_search_session(Nip50SearchSession::new(first, "native-search"));
        assert!(
            app.search_session_snapshot_bytes(&first_handle).is_some(),
            "initial search sidecar should be registered"
        );

        let second_handle =
            app.open_search_session(Nip50SearchSession::new(second, "native-search"));
        app.close_search_session(&first_handle);
        assert!(
            app.search_session_snapshot_bytes(&second_handle).is_some(),
            "stale typed close must not remove the replacement session"
        );

        app.close_search_session(&Nip50SearchHandle::for_key("native-search"));
        assert!(
            app.search_session_snapshot_bytes(&second_handle).is_none(),
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
/// must not bypass the cap any more than a hand-crafted search-session payload can.
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
