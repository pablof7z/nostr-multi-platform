//! Crate-registered full-text search scopes (issue #1811).
//!
//! This is the **protocol-aware** half of the FTS design. `nmp-store` owns the
//! noun-free vocabulary (`SearchScopeId`, `SearchField`, `TextSearchQuery`,
//! `CompiledIndexSpec`, the shared tokenizer) and the storage/query primitive
//! [`text_search_visit`](nmp_store::EventStore::text_search_visit). It does NOT
//! depend on `nmp-core` and never names a protocol concept (D0).
//!
//! `nmp-core` owns the registry here: protocol / app crates implement
//! [`SearchScopeProvider`] to declare *which* kinds and fields make up a
//! searchable scope, its privacy policy, and whether it has cache and/or relay
//! search. At composition time the registry COMPILES every provider into a
//! noun-free [`nmp_store::CompiledIndexSpec`] (dropping `LocalOnlyPrivate`
//! scopes and private/encrypted kinds) and installs the set into the store via
//! [`nmp_store::EventStore::install_search_index_specs`]. The store then runs
//! only the opaque extractor + the shared tokenizer.
//!
//! Registration follows the explicit composition-root house style
//! (ADR-0046 / ADR-0049 — NO linkme/inventory): a crate registers through the
//! [`SearchScopeRegistrar`] trait on the `AppHost`, the registry records a
//! [`crate::Disposition`] in the `"search_scope"` composition-ledger seam, and
//! a duplicate scope id is a **yielding default** (the first registration keeps
//! the slot; the later one is recorded as `YieldedToExisting`, never silently
//! replaced — ADR-0049).

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use crate::store::{CompiledIndexSpec, EventStore, SearchField, SearchScopeId, StoredEvent};

/// Whether a scope's documents may be served by generic public search and
/// fanned out to relays, or are local-only/private.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchPrivacyPolicy {
    /// Public, indexable, and (if the scope declares relay search) fannable to
    /// NIP-50 relays. Compiled into the store's public index.
    PublicIndexable,
    /// Local-only/private: indexed for cache search ONLY, never fanned out to
    /// relays by default. Dropped from the public install set by the compiler
    /// (Phase 1); a future opt-in install path will carry these as
    /// `local_only_private = true`.
    LocalOnlyPrivate,
}

/// Where a scope's search runs: cache (local FTS), relays (NIP-50 fanout), or
/// both. Owned by the registering crate, not the kernel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheSearchMode {
    /// Cache-only — local FTS, no relay fanout (e.g. a NIP-60 wallet scope).
    CacheOnly,
    /// Relay-only — NIP-50 fanout, no local cache index.
    RelayOnly,
    /// Both cache FTS and relay fanout.
    Both,
}

/// A protocol-aware searchable-scope declaration. The owning crate fills this
/// in; the kernel never authors one.
#[derive(Clone, Debug)]
pub struct SearchIndexSpec {
    /// Stable scope identity. Construct from a `&'static str` label via
    /// [`nmp_store::SearchScopeId::from_label`] so two crates can't collide on a
    /// hand-picked integer.
    pub scope: SearchScopeId,
    /// Human-readable description of the document source (diagnostics only).
    pub source: &'static str,
    /// The kinds this scope indexes (e.g. `{0}` for profiles, `{1}` for notes).
    pub kinds: BTreeSet<u32>,
    /// Declared indexable fields (field id + weight). The meaning of each field
    /// id is the owning crate's concern; the store only sees `(field, text)`.
    pub fields: Vec<SearchField>,
    pub privacy: SearchPrivacyPolicy,
    pub cache_mode: CacheSearchMode,
}

/// A crate-registered searchable scope: its spec + a field extractor.
///
/// Protocol crates (`nmp-nip50` for profiles/notes/long-form, `nmp-nip29` for
/// groups, a future `nmp-nip60` for wallet objects) implement this. The
/// extractor is opaque to the store: given a stored event it returns the
/// `(field, text)` pairs to tokenize.
pub trait SearchScopeProvider: Send + Sync {
    /// The scope's protocol-aware specification.
    fn spec(&self) -> SearchIndexSpec;

    /// Extract the `(field, text)` pairs to index for `event`. Called by the
    /// store at ingest for events whose kind is in the scope's `kinds`.
    fn extract(&self, event: &StoredEvent) -> Vec<(SearchField, String)>;
}

/// Register a [`SearchScopeProvider`] against the host. A narrow registration
/// trait (D6 / D26): a protocol crate takes `&impl SearchScopeRegistrar`, never
/// the whole `AppHost`.
pub trait SearchScopeRegistrar {
    /// Register `provider`. Pre-start, additive; a duplicate scope id yields
    /// (ADR-0049 — first registration wins, the later one is recorded as
    /// `YieldedToExisting` in the `"search_scope"` ledger seam).
    fn register_search_scope(&self, provider: Arc<dyn SearchScopeProvider>);
}

/// Outcome of one [`SearchScopeRegistry::register`] call (mirrors the
/// composition-ledger dispositions so the FFI shell can record it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchScopeDisposition {
    /// First registration for this scope id.
    Installed,
    /// A later registration for an already-claimed scope id — yielded
    /// (ADR-0049). The existing provider keeps the scope.
    YieldedToExisting,
}

#[inline]
fn is_private_kind(kind: u32) -> bool {
    nmp_kinds::is_private_relay_provenance_kind(kind)
}

/// The crate-registered scope registry. Lives behind a `Mutex` so a shared
/// `Arc` can be handed to the host registration surface; compiled + installed
/// into the store once at composition time.
#[derive(Default)]
pub struct SearchScopeRegistry {
    providers: Mutex<Vec<Arc<dyn SearchScopeProvider>>>,
}

impl SearchScopeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a provider. Yields (ADR-0049) on a duplicate scope id: the
    /// first registration keeps the scope; a later one for the same id is NOT
    /// installed and returns [`SearchScopeDisposition::YieldedToExisting`]. The
    /// caller (FFI shell) records the disposition in the `"search_scope"`
    /// ledger seam.
    pub fn register(&self, provider: Arc<dyn SearchScopeProvider>) -> SearchScopeDisposition {
        let scope = provider.spec().scope;
        let Ok(mut providers) = self.providers.lock() else {
            // D6 — a poisoned lock drops the registration silently.
            return SearchScopeDisposition::YieldedToExisting;
        };
        if providers.iter().any(|p| p.spec().scope == scope) {
            return SearchScopeDisposition::YieldedToExisting;
        }
        providers.push(provider);
        SearchScopeDisposition::Installed
    }

    /// Number of registered providers (diagnostics / tests).
    #[must_use]
    pub fn len(&self) -> usize {
        self.providers.lock().map(|p| p.len()).unwrap_or(0)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Compile every registered provider into a noun-free
    /// [`CompiledIndexSpec`], dropping `LocalOnlyPrivate` scopes, `RelayOnly`
    /// scopes (no cache index), and private/encrypted kinds (D10). Scopes left
    /// with no indexable kinds are dropped.
    #[must_use]
    pub fn compile(&self) -> Vec<CompiledIndexSpec> {
        let Ok(providers) = self.providers.lock() else {
            return Vec::new();
        };
        providers
            .iter()
            .filter_map(|provider| compile_one(provider))
            .collect()
    }

    /// Compile + install the registered scopes into `store`. Called once at
    /// composition time, after the store exists (kernel construction). A
    /// `RelayOnly` / empty registry installs an empty set (the store's text
    /// search then returns `Unsupported` for those scopes).
    pub fn install_into(&self, store: &dyn EventStore) {
        store.install_search_index_specs(self.compile());
    }
}

/// The registry is itself a [`SearchScopeRegistrar`]: a protocol crate's
/// `register_search_scopes(host: &impl SearchScopeRegistrar)` helper can be
/// driven directly against a bare `SearchScopeRegistry` (composition roots /
/// integration harnesses that hold the registry without an `AppHost`). The
/// `AppHost`/FFI shell forwards to the same `register` method, so the
/// ADR-0049 yield semantics are identical on both paths.
impl SearchScopeRegistrar for SearchScopeRegistry {
    fn register_search_scope(&self, provider: Arc<dyn SearchScopeProvider>) {
        let _ = self.register(provider);
    }
}

/// Compile one provider, returning `None` when it contributes no public cache
/// index (LocalOnlyPrivate, RelayOnly, or all-private-kinds).
fn compile_one(provider: &Arc<dyn SearchScopeProvider>) -> Option<CompiledIndexSpec> {
    let spec = provider.spec();
    // Drop scopes that do not produce a public cache index in Phase 1.
    if spec.privacy == SearchPrivacyPolicy::LocalOnlyPrivate {
        return None;
    }
    if spec.cache_mode == CacheSearchMode::RelayOnly {
        return None;
    }
    // Drop private/encrypted kinds (D10).
    let kinds: BTreeSet<u32> = spec
        .kinds
        .iter()
        .copied()
        .filter(|k| !is_private_kind(*k))
        .collect();
    if kinds.is_empty() {
        return None;
    }
    let provider = Arc::clone(provider);
    let extract =
        Arc::new(move |event: &StoredEvent| provider.extract(event)) as Arc<nmp_store::ExtractFn>;
    Some(CompiledIndexSpec {
        scope_id: spec.scope,
        kinds,
        extract,
        local_only_private: false,
    })
}

#[cfg(test)]
#[path = "search/tests.rs"]
mod tests;
