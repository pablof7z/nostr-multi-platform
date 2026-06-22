//! Cache-side full-text search seam (issue #1811).
//!
//! This module is the noun-free FTS "spine" shared by every `EventStore`
//! backend. It owns:
//!
//! * the value vocabulary ([`types`]) — `SearchScopeId`, `SearchField`,
//!   `TextSearchQuery`, `TextSearchHit`, `TextSearchStatus`, `CompiledIndexSpec`;
//! * the shared [`tokenizer`] (NFKC + Unicode-lowercase + alphanumeric split +
//!   short-token drop + per-doc cap) reused at index time AND query time so a
//!   token written at ingest is found at query time.
//!
//! Matching is **token + prefix** (issue #1811 locked decision): a multi-token
//! query is AND-combined; all but the trailing token match an indexed token
//! exactly, and the trailing token matches by prefix (typeahead). NOT substring,
//! NOT stemming/fuzzy.
//!
//! Layering (D0, load-bearing): this crate stays protocol-noun-free. `nmp-core`
//! owns the protocol-aware `SearchIndexSpec`/`SearchScopeProvider`, compiles
//! them into [`CompiledIndexSpec`], and installs them via
//! [`crate::EventStore::install_search_index_specs`].

pub mod tokenizer;
pub mod types;

pub use tokenizer::{
    is_prefix_match, split_query_terms, tokenize, MAX_TOKENS_PER_DOC, MIN_TOKEN_BYTES,
    TOKENIZER_VERSION,
};
pub use types::{
    CompiledIndexSpec, ExtractFn, SearchDocumentKey, SearchField, SearchScopeId, SearchScore,
    TextSearchBudget, TextSearchHit, TextSearchOrder, TextSearchQuery, TextSearchStatus,
};
