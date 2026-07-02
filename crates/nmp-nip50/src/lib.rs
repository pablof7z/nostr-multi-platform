//! `nmp-nip50` — NIP-50 search request and result projection primitives.
//!
//! The planner/core substrate owns only the generic `search` filter field and
//! wire serialization. This crate owns NIP-50 query scopes, the public
//! searchable-scope declarations (profiles / notes / long-form — issue #1811),
//! and the bounded deduplicating result projection.
//!
//! This crate is not the generic user-input resolver. Direct NIP-19/NIP-21
//! references, NIP-05 identifiers, relay URLs, and crate-registered domains
//! such as NIP-29 groups are classified before search and routed through their
//! existing seams; only free-text search requests enter `SearchRequest`.

mod input_recognizers;
mod projection;
mod request;
mod scopes;

// Higher-order `open_search` orchestration (relay resolution + per-relay
// relay-pinned interest plan + the host registration seam) and the typed N50S
// FlatBuffers sidecar codec for `SearchResultsSnapshot`. The substrate owns the
// generic `InterestShape.search` wire field; these modules own NIP-50 relay
// selection, the dedup result projection's transparent wiring, and the typed
// snapshot transport.
pub mod search;
pub mod wire;

pub use input_recognizers::{
    LongFormInputRecognizer, NotesInputRecognizer, ProfilesInputRecognizer,
};
pub use projection::{SearchHit, SearchHitSource, SearchResultsProjection, SearchResultsSnapshot};
pub use request::{
    SearchRequest, SearchScope, SearchTargets, DEFAULT_MAX_SEARCH_HITS, HARD_MAX_SEARCH_HITS,
};
pub use scopes::{
    LongFormSearchScope, NoteSearchScope, ProfileSearchScope, SCOPE_LABEL_LONGFORM,
    SCOPE_LABEL_NOTES, SCOPE_LABEL_PROFILES, SCOPE_NAME_LONGFORM, SCOPE_NAME_NOTES,
    SCOPE_NAME_PROFILES,
};
pub use search::{
    install_search_relay_source, resolve_search_relays, search_relay_plan, RelayPinnedInterest,
    SearchFallbackRelays, SearchRelaySource, SearchSessionBuild, SearchSessionRegistry,
    SearchTeardownAction,
};
pub use wire::{
    decode_search_results_snapshot, encode_search_results_snapshot,
    FILE_IDENTIFIER as SEARCH_RESULTS_FILE_IDENTIFIER, SCHEMA_ID as SEARCH_RESULTS_SCHEMA_ID,
    SCHEMA_VERSION as SEARCH_RESULTS_SCHEMA_VERSION,
};

#[derive(Clone, Debug, Default)]
pub struct Config {}

#[derive(Clone, Debug, Default)]
pub struct Handles {}

pub fn register(
    app: &(impl nmp_core::substrate::SearchScopeRegistrar + nmp_core::substrate::InputScopeRegistrar),
    _config: Config,
) -> Result<Handles, nmp_core::substrate::RegistrationError> {
    scopes::register_search_scopes(app);
    input_recognizers::register_input_scopes(app);
    Ok(Handles {})
}

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
