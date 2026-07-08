//! Lane-mapping registry (#3082 settled design).
//!
//! A [`crate::LaneMappingId`] crosses FFI as an opaque registered id. The
//! closure it names is a [`LaneMapping`] — a pure function of the delivered
//! event, constructed in Rust at the composition root and never crossing FFI
//! (same discipline as `CustomAdmissionId`/`CustomSourceId`). Protocol crates
//! register extraction mappings under framework-owned ids (`nip18.target`,
//! `nip22.root`); `nmp-feed` registers only the kind-blind identity mapping
//! (`feed.authored`). The engine never learns a kind — this registry is the
//! ONLY place kind-specific extraction logic lives, and it lives in the
//! PROTOCOL crate that owns the kind, not in the compiler.
//!
//! This lives in `nmp-feed` (not the higher `nmp-feed-session` compiler layer)
//! so protocol crates (`nmp-nip18`, `nmp-nip22`, ...) — which sit BELOW
//! `nmp-feed-session` in the dependency graph — can depend on it without a
//! cycle. Registration is register-once (immutable), the same fail-open-drift
//! protection [`crate::CustomFeedPolicyRegistry`] uses.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::KernelEvent;

use crate::composite::{LaneMappingId, DIRECT_MAPPING_ID};
use crate::feed_row::FeedRowContext;
use crate::typed_ref::TypedRef;

/// Whether a [`MappedRow`]'s payload should reflect the triggering event's own
/// raw fields, or stay an un-hydrated placeholder pending its `Delivered` ref
/// target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MappedPayload {
    /// Use the triggering event's own raw fields (a `Direct`/authored-style
    /// mapping, or a delivered-target admission).
    FromEvent,
    /// Placeholder (kind `0`, empty fields) — provenance-only until the
    /// `Delivered` ref target arrives.
    Placeholder,
}

/// One row a [`LaneMapping`] produces for a matched event.
pub struct MappedRow {
    pub canonical_row_id: String,
    pub payload: MappedPayload,
    pub context: Vec<FeedRowContext>,
    pub refs: Vec<TypedRef>,
}

/// A registered lane-mapping closure: a pure function of the delivered event.
///
/// No store peek, no ambient state — the same determinism contract as
/// [`nmp_feed::FlatFeedItemBuilder`] (arity `Vec`: a mapping may fan one event
/// into zero, one, or many rows).
pub type LaneMapping = Arc<dyn Fn(&KernelEvent) -> Vec<MappedRow> + Send + Sync>;

/// Register-once lane-mapping registry. Composition roots register protocol
/// extractors (`nip18.target`, `nip22.root`, ...) plus `nmp-feed`'s own
/// `feed.authored` identity mapping into one instance shared across composite
/// feed opens.
#[derive(Default)]
pub struct LaneMappingRegistry {
    mappings: Mutex<BTreeMap<LaneMappingId, LaneMapping>>,
}

impl LaneMappingRegistry {
    #[must_use]
    pub fn new() -> Self {
        let registry = Self::default();
        registry.register(
            LaneMappingId(DIRECT_MAPPING_ID.to_string()),
            direct_mapping(),
        );
        registry
    }

    /// Register `mapping` under `id`. Returns `false` (no-op) if `id` is
    /// already registered — register-once immutability (no fail-open drift
    /// where an already-open composite feed keeps using a stale mapping after
    /// a later registration silently replaced it).
    pub fn register(&self, id: LaneMappingId, mapping: LaneMapping) -> bool {
        let Ok(mut mappings) = self.mappings.lock() else {
            return false;
        };
        if mappings.contains_key(&id) {
            return false;
        }
        mappings.insert(id, mapping);
        true
    }

    #[must_use]
    pub fn get(&self, id: &LaneMappingId) -> Option<LaneMapping> {
        self.mappings.lock().ok().and_then(|m| m.get(id).cloned())
    }
}

/// `nmp-feed`'s own kind-blind identity mapping (`feed.authored`,
/// [`DIRECT_MAPPING_ID`]): `canonical_row_id = event.id`, payload from the
/// event itself, `Authored` provenance. This is the zero-config default lane
/// mapping ([`nmp_feed::FeedLane::direct`]) — "render the source" holds by
/// construction.
fn direct_mapping() -> LaneMapping {
    Arc::new(|event: &KernelEvent| {
        vec![MappedRow {
            canonical_row_id: event.id.clone(),
            payload: MappedPayload::FromEvent,
            context: vec![FeedRowContext::Authored],
            refs: Vec::new(),
        }]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::EventId;

    fn event(id: &str) -> KernelEvent {
        KernelEvent {
            id: EventId::from(id),
            author: "author".to_string(),
            kind: 30_023,
            created_at: 100,
            tags: Vec::new(),
            content: "hello".to_string(),
            relay_provenance: Vec::new(),
        }
    }

    #[test]
    fn registry_preinstalls_the_direct_mapping() {
        let registry = LaneMappingRegistry::new();
        let mapping = registry
            .get(&LaneMappingId(DIRECT_MAPPING_ID.to_string()))
            .expect("direct mapping registered");
        let rows = mapping(&event("a"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].canonical_row_id, "a");
        assert_eq!(rows[0].context, vec![FeedRowContext::Authored]);
        assert!(matches!(rows[0].payload, MappedPayload::FromEvent));
    }

    #[test]
    fn register_once_rejects_a_second_registration_under_the_same_id() {
        let registry = LaneMappingRegistry::default();
        let id = LaneMappingId("test.mapping".to_string());
        assert!(registry.register(id.clone(), direct_mapping()));
        assert!(!registry.register(id, direct_mapping()), "register-once");
    }
}
