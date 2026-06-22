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

mod projection;
mod request;
mod scopes;

pub use projection::{
    SearchHit, SearchHitSource, SearchResultsProjection, SearchResultsSnapshot,
};
pub use request::{
    SearchRequest, SearchScope, SearchTargets, DEFAULT_MAX_SEARCH_HITS, HARD_MAX_SEARCH_HITS,
    KIND_LONG_FORM,
};
pub use scopes::{
    register_search_scopes, LongFormSearchScope, NoteSearchScope, ProfileSearchScope,
    SCOPE_LABEL_LONGFORM, SCOPE_LABEL_NOTES, SCOPE_LABEL_PROFILES,
};
