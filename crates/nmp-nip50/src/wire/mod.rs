//! Typed FlatBuffers wire codec for `nmp-nip50` search-results projection
//! (ADR-0037, S9).
//!
//! The serde JSON shape of [`crate::SearchResultsSnapshot`] stays the in-crate
//! authority; this module adds the typed-sidecar (`N50S`) counterpart the host
//! platforms decode with generated accessors. `open_search` registers exactly
//! this buffer per live search session.

pub mod search_results_fb;

pub use search_results_fb::{
    decode_search_results_snapshot, encode_search_results_snapshot, FILE_IDENTIFIER, SCHEMA_ID,
    SCHEMA_VERSION,
};
