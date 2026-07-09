//! Kind-blind repost-target resolution.
//!
//! Every `FlatFeed`-style repost consumer (the nip23 long-form feed, the
//! nip68 picture feed, and any future NIP-18 wrapper feed built the same way)
//! needs the SAME "embedded JSON, else a local synchronous lookup, else
//! drop" resolution before it can even attempt to render a repost's target.
//! That resolution is generic repost mechanics — `nmp-nip18` owns it, not
//! any target kind's semantics (#3100). Consumers apply their OWN
//! kind-specific mapping and any additional admission predicate (e.g. a
//! topic filter) to the resolved [`KernelEvent`] on top of this.
//!
//! This is distinct from the [`crate::nip18_target_render_only_mapping`]
//! composite-engine lane mapping: that mapping declares a lazily-resolved
//! `RenderOnly` [`nmp_feed::TypedRef`] and hydrates a
//! [`nmp_feed::MappedPayload`] straight from the embedded fields — it never
//! performs a synchronous local lookup, and it never needs a
//! [`KernelEvent`]-shaped target. This module's synchronous
//! embedded-else-lookup resolution is for feeds that have not (yet) moved
//! onto that composite engine.

use nmp_core::substrate::KernelEvent;
use nmp_feed::EventLookup;

use crate::RepostRecord;

/// Resolve a repost wrapper's target event.
///
/// Resolution order, never fetching over the network:
/// 1. The wrapper's embedded JSON payload, if present — rebuilt into a
///    [`KernelEvent`] whose `relay_provenance` is inherited from the
///    *wrapper* (`wrapper.relay_provenance`), since the target itself was
///    never independently received over the wire in this case.
/// 2. Otherwise, the wrapper's proven target event id passed through the
///    caller-supplied local `event_lookup`.
/// 3. Otherwise `None` — an unresolvable target is dropped, never guessed.
#[must_use]
pub fn resolve_repost_target(
    record: &RepostRecord,
    wrapper: &KernelEvent,
    event_lookup: &EventLookup,
) -> Option<KernelEvent> {
    if let Some(embedded) = record.embedded_event.clone() {
        return Some(KernelEvent {
            id: embedded.id,
            author: embedded.author,
            kind: embedded.kind,
            created_at: embedded.created_at,
            tags: embedded.tags,
            content: embedded.content,
            relay_provenance: wrapper.relay_provenance.clone(),
        });
    }
    record
        .target_event_id
        .as_ref()
        .and_then(|target_id| (event_lookup)(target_id))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{try_from_kernel_event, KIND_GENERIC_REPOST};

    fn wrapper(tags: Vec<Vec<&str>>, content: &str) -> KernelEvent {
        KernelEvent {
            id: "wrapper".to_string(),
            author: "reposter".to_string(),
            kind: KIND_GENERIC_REPOST,
            created_at: 200,
            tags: tags
                .into_iter()
                .map(|tag| tag.into_iter().map(str::to_string).collect())
                .collect(),
            content: content.to_string(),
            relay_provenance: vec!["wss://wrapper.example".to_string()],
        }
    }

    #[test]
    fn resolves_from_embedded_json_and_inherits_wrapper_provenance() {
        let content = serde_json::json!({
            "id": "inner",
            "pubkey": "target-author",
            "kind": 1,
            "created_at": 123,
            "tags": [["t", "nostr"]],
            "content": "hello from the embed",
            "sig": "ignored",
        })
        .to_string();
        let wrapper_event = wrapper(vec![vec!["e", "inner"]], &content);
        let record = try_from_kernel_event(&wrapper_event).unwrap();

        let no_lookup: EventLookup = Arc::new(|_: &String| None);
        let resolved = resolve_repost_target(&record, &wrapper_event, &no_lookup).unwrap();

        assert_eq!(resolved.id, "inner");
        assert_eq!(resolved.author, "target-author");
        assert_eq!(resolved.created_at, 123);
        assert_eq!(
            resolved.relay_provenance,
            vec!["wss://wrapper.example".to_string()],
            "embedded target inherits the WRAPPER's own provenance"
        );
    }

    #[test]
    fn falls_back_to_event_lookup_when_nothing_is_embedded() {
        let wrapper_event = wrapper(vec![vec!["e", "target-id"]], "");
        let record = try_from_kernel_event(&wrapper_event).unwrap();
        let looked_up = KernelEvent {
            id: "target-id".to_string(),
            author: "target-author".to_string(),
            kind: 1,
            created_at: 999,
            tags: Vec::new(),
            content: "local".to_string(),
            relay_provenance: Vec::new(),
        };

        let event_lookup: EventLookup = Arc::new(move |id: &String| {
            (id == "target-id").then(|| looked_up.clone())
        });
        let resolved = resolve_repost_target(&record, &wrapper_event, &event_lookup).unwrap();

        assert_eq!(resolved.author, "target-author");
        assert_eq!(resolved.created_at, 999);
    }

    #[test]
    fn drops_when_target_is_neither_embedded_nor_locally_known() {
        let wrapper_event = wrapper(vec![vec!["e", "missing"]], "");
        let record = try_from_kernel_event(&wrapper_event).unwrap();

        let no_lookup: EventLookup = Arc::new(|_: &String| None);
        let resolved = resolve_repost_target(&record, &wrapper_event, &no_lookup);

        assert!(resolved.is_none());
    }
}
