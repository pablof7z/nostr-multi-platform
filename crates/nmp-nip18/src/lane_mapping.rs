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
    DeliveryMode, FeedRowContext, LaneMapping, MappedFields, MappedPayload, MappedRow, TypedRef,
    TypedRefTarget,
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

/// The single-lane follows-timeline's `nip18.target` mapping (#3092 —
/// collapses `nmp-note-feed`'s bespoke repost builder/merge onto this SAME
/// composite lane-mapping engine).
///
/// Row identity/provenance are IDENTICAL to [`nip18_target_mapping`] (target
/// id/coordinate, `RepostedBy` context). Two things differ, both preserving
/// the exact pre-#3082 follows-timeline behavior:
///
/// * the target ref is declared `RenderOnly`, not `Delivered` — the
///   single-lane follows timeline never actively widens its OWN acquisition
///   to fetch an arbitrary repost target; a shell resolves it lazily via
///   `resolve_ref` instead (unchanged D7 behavior). Merging in the target's
///   real content still happens if the target is ALSO independently admitted
///   by this session's own acquisition (e.g. its author is also followed) —
///   see `composite_merge`'s `ByInteractionTime` policy.
/// * when the wrapper EMBEDS its target's JSON (the common NIP-18 case), the
///   row hydrates immediately from that wrapper-local payload
///   ([`MappedPayload::Explicit`]) instead of waiting on a second delivery —
///   this is parsing the wrapper's own content, never a store peek.
#[must_use]
pub fn nip18_target_render_only_mapping() -> LaneMapping {
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
            // Unlike `nip18_target_mapping`, an unproven target does not drop
            // the row: the pre-#3082 follows-timeline builder always keyed a
            // valid repost event by SOME id, falling back to the wrapper's
            // own — preserved here exactly (#3092).
            None => TypedRefTarget::EventId(
                repost
                    .target_event_id
                    .clone()
                    .unwrap_or_else(|| event.id.clone()),
            ),
        };
        let note_created_at = repost
            .embedded_event
            .as_ref()
            .map_or(event.created_at, |inner| inner.created_at);
        let context = vec![FeedRowContext::RepostedBy {
            author_pubkey: event.author.clone(),
            note_created_at,
        }];
        let refs = vec![TypedRef {
            target: target.clone(),
            delivery_mode: DeliveryMode::RenderOnly,
        }];
        let payload = match &repost.embedded_event {
            Some(inner) => MappedPayload::Explicit(MappedFields {
                author_pubkey: inner.author.clone(),
                kind: inner.kind,
                content: inner.content.clone(),
                tags: inner.tags.clone(),
                created_at: inner.created_at,
            }),
            None => MappedPayload::Placeholder,
        };
        vec![MappedRow {
            canonical_row_id: target.canonical_key(),
            payload,
            context,
            refs,
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

    fn embedded_repost(id: &str, target_id: &str, target_json: &str, created_at: u64) -> KernelEvent {
        KernelEvent {
            id: EventId::from(id),
            author: "reposter".to_string(),
            kind: crate::KIND_REPOST,
            created_at,
            tags: vec![vec!["e".to_string(), target_id.to_string()]],
            content: target_json.to_string(),
            relay_provenance: Vec::new(),
        }
    }

    #[test]
    fn render_only_mapping_declares_a_render_only_ref_not_delivered() {
        let mapping = nip18_target_render_only_mapping();
        let rows = mapping(&repost_event("wrapper", vec![vec!["e", "target-id"]]));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].canonical_row_id, "target-id");
        assert_eq!(
            rows[0].refs,
            vec![TypedRef {
                target: TypedRefTarget::EventId("target-id".to_string()),
                delivery_mode: DeliveryMode::RenderOnly,
            }],
            "the single-lane follows timeline never actively fetches a repost target"
        );
        assert!(matches!(rows[0].payload, MappedPayload::Placeholder));
    }

    #[test]
    fn render_only_mapping_hydrates_immediately_from_embedded_json() {
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
        let mapping = nip18_target_render_only_mapping();
        let rows = mapping(&embedded_repost("wrapper", "inner", &content, 200));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].canonical_row_id, "inner");
        match &rows[0].payload {
            MappedPayload::Explicit(fields) => {
                assert_eq!(fields.author_pubkey, "target-author");
                assert_eq!(fields.kind, 1);
                assert_eq!(fields.content, "hello from the embed");
                assert_eq!(fields.created_at, 123);
            }
            other => panic!("expected Explicit payload, got {other:?}"),
        }
        assert_eq!(
            rows[0].context,
            vec![FeedRowContext::RepostedBy {
                author_pubkey: "reposter".to_string(),
                note_created_at: 123,
            }],
            "provenance carries the embedded target's own created_at"
        );
    }

    #[test]
    fn render_only_mapping_falls_back_to_the_wrapper_own_id_when_no_target_is_proven() {
        // A valid repost kind with no `e`/`a` tag and no embedded content still
        // produces a row, keyed by the wrapper's own id — the pre-#3082
        // follows-timeline builder's exact fallback (#3092).
        let mapping = nip18_target_render_only_mapping();
        let rows = mapping(&repost_event("wrapper", Vec::new()));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].canonical_row_id, "wrapper");
        assert!(matches!(rows[0].payload, MappedPayload::Placeholder));
    }
}
