//! `nip18.target` composite-feed lane mapping (#3082 settled design).
//!
//! Canonical row identity = the reposted TARGET's own id/coordinate, computed
//! purely from the wrapper's OWN tags/embedded content — wrapper-local, never
//! a store peek at the target (the #3083 cache-luck bug class this design
//! forecloses). Provenance = [`nmp_feed::FeedRowContext::RepostedBy`]. The
//! target is declared as a `Delivered` ref: the composite compiler
//! (`nmp-feed-session`) folds its key into the SAME feed session's own
//! admission + live shapes, so the target re-enters `on_kernel_event` as a
//! real delivered event carrying its true `created_at` and its own
//! `Authored` provenance contribution.
//!
//! This mapping produces a PLACEHOLDER payload (kind `0`) — it never guesses
//! the target's content; the row stays a placeholder until the target itself
//! is delivered and the composite merge policy adopts its real payload.

use std::sync::Arc;

use nmp_core::substrate::KernelEvent;
use nmp_feed::{
    DeliveryMode, FeedRowContext, LaneMapping, MappedPayload, MappedRow, TypedRef, TypedRefTarget,
};

/// The registered lane-mapping id this crate owns.
pub const NIP18_TARGET_MAPPING_ID: &str = "nip18.target";

/// Build the `nip18.target` lane mapping.
#[must_use]
pub fn nip18_target_mapping() -> LaneMapping {
    Arc::new(|event: &KernelEvent| {
        let Some(repost) = crate::try_from_kernel_event(event) else {
            return Vec::new();
        };
        let target = match repost.target_address.clone() {
            Some(address) => TypedRefTarget::Address {
                kind: address.kind,
                pubkey: address.pubkey,
                d: address.identifier,
            },
            None => match repost.target_event_id.clone() {
                Some(id) => TypedRefTarget::EventId(id),
                // No proven target at all — this repost declares nothing.
                None => return Vec::new(),
            },
        };
        vec![MappedRow {
            canonical_row_id: target.canonical_key(),
            payload: MappedPayload::Placeholder,
            context: vec![FeedRowContext::RepostedBy {
                author_pubkey: event.author.clone(),
                note_created_at: event.created_at,
            }],
            refs: vec![TypedRef {
                target,
                delivery_mode: DeliveryMode::Delivered,
            }],
        }]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::EventId;

    fn repost_event(id: &str, tags: Vec<Vec<&str>>) -> KernelEvent {
        KernelEvent {
            id: EventId::from(id),
            author: "reposter".to_string(),
            kind: crate::KIND_GENERIC_REPOST,
            created_at: 200,
            tags: tags
                .into_iter()
                .map(|tag| tag.into_iter().map(str::to_string).collect())
                .collect(),
            content: String::new(),
            relay_provenance: Vec::new(),
        }
    }

    #[test]
    fn address_target_canonical_matches_the_typed_ref_target_key() {
        let mapping = nip18_target_mapping();
        let rows = mapping(&repost_event(
            "wrapper",
            vec![vec!["a", "30023:author:article"], vec!["k", "30023"]],
        ));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].canonical_row_id, "30023:author:article");
        assert!(matches!(rows[0].payload, MappedPayload::Placeholder));
        assert_eq!(
            rows[0].context,
            vec![FeedRowContext::RepostedBy {
                author_pubkey: "reposter".to_string(),
                note_created_at: 200,
            }]
        );
        assert_eq!(
            rows[0].refs,
            vec![TypedRef {
                target: TypedRefTarget::Address {
                    kind: 30_023,
                    pubkey: "author".to_string(),
                    d: "article".to_string(),
                },
                delivery_mode: DeliveryMode::Delivered,
            }]
        );
    }

    #[test]
    fn event_id_target_is_used_when_no_address_is_proven() {
        let mapping = nip18_target_mapping();
        let rows = mapping(&repost_event("wrapper", vec![vec!["e", "target-id"]]));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].canonical_row_id, "target-id");
    }

    #[test]
    fn non_repost_event_produces_no_rows() {
        let mut event = repost_event("note", Vec::new());
        event.kind = 1;
        assert!(nip18_target_mapping()(&event).is_empty());
    }
}
