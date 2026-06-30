//! Noun-free full-text-search value types (issue #1811).
//!
//! These types are the store's text-search vocabulary. They name NO protocol
//! concept (D0): a "scope" is an opaque `u32` discriminant, a "field" is a
//! small numeric id, a "document" is a 32-byte key. `nmp-core` owns the
//! protocol-aware `SearchIndexSpec`/`SearchScopeProvider` and COMPILES them
//! into the store-local [`CompiledIndexSpec`] below; the store only runs the
//! opaque extractor + the shared tokenizer.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::types::{EventId, StoredEvent};

/// Identifier for a registered search scope.
///
/// `discriminant` is a stable numeric id (the compiler in `nmp-core` derives it
/// from the scope's `&'static str` label via FNV-1a so two crates can't collide
/// on a hand-picked integer); `label` is the human-readable scope name kept for
/// diagnostics / ledger records only. Equality / ordering / hashing key on the
/// discriminant alone so the label never affects index keying.
#[derive(Clone, Copy, Debug)]
pub struct SearchScopeId {
    discriminant: u32,
    label: &'static str,
}

impl SearchScopeId {
    /// Construct from an explicit discriminant + label. `nmp-core`'s scope
    /// compiler is the normal caller (it hashes the label to the discriminant);
    /// tests may construct directly.
    #[must_use]
    pub const fn new(discriminant: u32, label: &'static str) -> Self {
        Self {
            discriminant,
            label,
        }
    }

    /// FNV-1a 32-bit hash of `label` → discriminant. Documented, stable, and
    /// reachable without a dependency on `nmp-core`'s `stable_hash64`, so the
    /// store and the compiler agree on the same id for a given label.
    #[must_use]
    pub const fn from_label(label: &'static str) -> Self {
        let bytes = label.as_bytes();
        let mut hash: u32 = 0x811c_9dc5; // FNV offset basis
        let mut i = 0;
        while i < bytes.len() {
            hash ^= bytes[i] as u32;
            hash = hash.wrapping_mul(0x0100_0193); // FNV prime
            i += 1;
        }
        Self {
            discriminant: hash,
            label,
        }
    }

    #[must_use]
    pub const fn discriminant(self) -> u32 {
        self.discriminant
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        self.label
    }
}

impl PartialEq for SearchScopeId {
    fn eq(&self, other: &Self) -> bool {
        self.discriminant == other.discriminant
    }
}
impl Eq for SearchScopeId {}
impl PartialOrd for SearchScopeId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for SearchScopeId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.discriminant.cmp(&other.discriminant)
    }
}
impl std::hash::Hash for SearchScopeId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.discriminant.hash(state);
    }
}

/// Opaque 32-byte key identifying an indexed document. For event-sourced scopes
/// this is the event id bytes; a scope MAY use any stable 32-byte key for a
/// domain row. The store treats it purely as an index key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SearchDocumentKey(pub [u8; 32]);

impl SearchDocumentKey {
    #[must_use]
    pub const fn from_event_id(id: EventId) -> Self {
        Self(id)
    }
}

/// A noun-free indexable field within a scope.
///
/// `id` distinguishes fields within one scope (e.g. a scope might separate
/// "title" weight from "body" weight); `weight` biases relevance scoring (higher
/// = more important). The store never interprets which field is which — that
/// meaning lives in the owning protocol crate's `SearchScopeProvider`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchField {
    pub id: u16,
    pub weight: u16,
}

impl SearchField {
    /// A field with the default weight (1).
    #[must_use]
    pub const fn new(id: u16) -> Self {
        Self { id, weight: 1 }
    }

    #[must_use]
    pub const fn with_weight(id: u16, weight: u16) -> Self {
        Self { id, weight }
    }
}

/// Relevance score for a hit. Higher is more relevant. Phase-1 mem backend
/// computes a simple field-weighted match count; the type is frozen so Phase-2
/// can refine the ranking without an API change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct SearchScore(pub u32);

/// Result ordering for a text search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSearchOrder {
    /// Newest `created_at` first (the default browse order).
    NewestFirst,
    /// Highest [`SearchScore`] first (Phase-2 target).
    ///
    /// **Phase-1 status**: both backends treat this as a recency-order proxy
    /// (newest-first) because no term-frequency data is stored in the posting
    /// index yet. The variant is intentionally kept frozen so Phase-2 can wire
    /// real TF-IDF scoring without an API break. Each backend's `order_candidates`
    /// / `emit` has an explicit arm with a `NOTE` comment acknowledging the gap.
    Relevance,
}

/// Bounded scan budget (D8 — a text search never degrades to a corpus-size
/// scan). The visit stops once either ceiling is reached and reports
/// [`TextSearchStatus::Partial`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextSearchBudget {
    /// Maximum number of candidate documents the plan may examine.
    pub max_docs_scanned: usize,
    /// Maximum number of matches collected before stopping.
    pub max_matches: usize,
}

impl Default for TextSearchBudget {
    fn default() -> Self {
        Self {
            max_docs_scanned: 10_000,
            max_matches: 1_000,
        }
    }
}

/// A text-search query over one registered scope.
///
/// `query` is the raw user text (tokenized by the store); `kinds` optionally
/// narrows to a subset of the scope's kinds (empty = all kinds the scope
/// indexes). `since`/`until` are inclusive unix-seconds bounds.
#[derive(Clone, Debug)]
pub struct TextSearchQuery {
    pub scope: SearchScopeId,
    pub query: String,
    pub kinds: BTreeSet<u32>,
    pub since: Option<u64>,
    pub until: Option<u64>,
    pub limit: usize,
    pub order: TextSearchOrder,
    pub budget: TextSearchBudget,
}

/// One matching document yielded to the visitor.
#[derive(Clone, Copy, Debug)]
pub struct TextSearchHit {
    pub doc: SearchDocumentKey,
    /// The source event id when the document is event-sourced (`Some` for every
    /// event-backed scope); `None` for a domain-row document not keyed by event.
    pub event_id: Option<EventId>,
    pub created_at: u64,
    pub score: SearchScore,
}

/// Terminal status of a [`text_search_visit`](crate::EventStore::text_search_visit)
/// call — the explicit diagnostic the caller surfaces in search UI state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSearchStatus {
    /// The full result set for the query was produced within budget/limit.
    Complete,
    /// The scan stopped early. `budget_exhausted` distinguishes "hit the scan
    /// budget" (more matches may exist) from "hit the `limit`" (caller asked
    /// for fewer than exist).
    Partial { budget_exhausted: bool },
    /// The backend does not support text search (or the scope is unknown).
    Unsupported,
    /// The index for this scope is still being built (Phase-2 LMDB backfill).
    IndexBuilding,
    /// A store-level error occurred mid-scan.
    StoreError,
}

/// The extractor signature: given a stored event, return the `(field, text)`
/// pairs to index for this scope. Opaque to the store — produced by the
/// owning protocol crate's `SearchScopeProvider::extract`, type-erased here so
/// the store never names the protocol concept.
pub type ExtractFn = dyn Fn(&StoredEvent) -> Vec<(SearchField, String)> + Send + Sync;

/// A protocol-noun-free, store-local compiled index specification.
///
/// `nmp-core` compiles each registered `SearchScopeProvider` into one of these
/// at composition time (dropping `LocalOnlyPrivate` scopes and private/encrypted
/// kinds) and installs the set via
/// [`install_search_index_specs`](crate::EventStore::install_search_index_specs).
/// The store runs `extract` + the shared tokenizer; it never names a protocol
/// concept.
#[derive(Clone)]
pub struct CompiledIndexSpec {
    pub scope_id: SearchScopeId,
    /// The kinds this scope indexes. An event whose kind is not in this set is
    /// never extracted for this scope.
    pub kinds: BTreeSet<u32>,
    /// Type-erased extractor (see [`ExtractFn`]).
    pub extract: Arc<ExtractFn>,
    /// When `true`, the scope is local-only/private: its documents are never
    /// served to a generic public search and the scope never fans out to relays.
    /// `nmp-core` drops `LocalOnlyPrivate` scopes from the public install set;
    /// this flag is retained so a backend can assert the invariant.
    pub local_only_private: bool,
}

impl std::fmt::Debug for CompiledIndexSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompiledIndexSpec")
            .field("scope_id", &self.scope_id)
            .field("kinds", &self.kinds)
            .field("local_only_private", &self.local_only_private)
            .finish_non_exhaustive()
    }
}
