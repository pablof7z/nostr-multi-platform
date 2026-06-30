//! #2512 — install the store-side classifiers at the browser composition root.
//!
//! The reducer kernel's store needs two crate-registered, protocol-noun-free
//! classifiers compiled into it once the store exists: the FTS scope registry
//! (#1811) and the cross-protocol engagement reference classifier
//! (`nmp-relations`, #2512). They are one cohesive "install crate-registered
//! specs into the reducer store" step, extracted here from `handle.rs` to keep
//! that file under the 500-LOC cap and to mirror the native `apply_to_kernel`
//! install site.

use nmp_core::substrate::SearchScopeRegistry;
use nmp_store::EventStore;

/// Install the FTS scope registry and the engagement reference classifier into
/// `store`.
///
/// The OPFS / sqlite-wasm backend ships no counter sidecar, so the engagement
/// install resolves to the trait-default no-op there — an accepted, documented
/// staged limitation (#2512); the seam stays live for when a wasm counter
/// sidecar or an injected `MemEventStore` lands.
pub(super) fn install_store_classifiers(registry: &SearchScopeRegistry, store: &dyn EventStore) {
    registry.install_into(store);
    nmp_relations::install_engagement_reference_counters(store);
}
