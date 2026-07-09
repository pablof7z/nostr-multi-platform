//! `nip22.root` composite-feed lane mapping (#3082 settled design).
//!
//! Canonical row identity = the comment's ROOT scope (uppercase `A`/`E`),
//! computed purely from the comment's OWN tags — wrapper-local, never a store
//! peek at the root. Provenance = [`nmp_feed::FeedRowContext::CommentedBy`].
//! The root is declared as a `Delivered` ref: the composite compiler
//! (`nmp-feed-session`) folds its key into the SAME feed session's own
//! admission + live shapes, so the root re-enters `on_kernel_event` as a real
//! delivered event carrying its true `created_at`.
//!
//! This mapping produces a PLACEHOLDER payload (kind `0`) — it never guesses
//! the root's content; the row stays a placeholder until the root itself is
//! delivered and the composite merge policy adopts its real payload.

use std::sync::Arc;

use nmp_core::substrate::KernelEvent;
use nmp_feed::{
    DeliveryMode, FeedRowContext, LaneMapping, MappedPayload, MappedRow, TypedRef, TypedRefTarget,
};

/// The registered lane-mapping id this crate owns.
pub const NIP22_ROOT_MAPPING_ID: &str = "nip22.root";

/// Build the `nip22.root` lane mapping.
#[must_use]
pub fn nip22_root_mapping() -> LaneMapping {
    Arc::new(|event: &KernelEvent| {
        let Some(comment) = crate::try_from_kernel_event(event) else {
            return Vec::new();
        };
        let Some(target) = root_target(&comment) else {
            return Vec::new();
        };
        vec![MappedRow {
            canonical_row_id: target.canonical_key(),
            payload: MappedPayload::Placeholder,
            context: vec![FeedRowContext::CommentedBy {
                author_pubkey: comment.author_pubkey.clone(),
                comment_event_id: comment.event_id.clone(),
                comment_created_at: comment.created_at,
            }],
            refs: vec![TypedRef {
                target,
                delivery_mode: DeliveryMode::Delivered,
            }],
        }]
    })
}

/// The comment's proven root target, from its own uppercase root scope tag.
/// `A` carries a NIP-01 `kind:pubkey:d` coordinate string; `E` carries a bare
/// event id. Any other/malformed root scope declares no target (fail closed —
/// never guess a coordinate from a partial tag).
fn root_target(comment: &crate::CommentRecord) -> Option<TypedRefTarget> {
    match comment.root_tag_name.as_str() {
        "A" => {
            let mut parts = comment.root_tag_value.splitn(3, ':');
            let kind: u32 = parts.next()?.parse().ok()?;
            let pubkey = parts.next()?.to_string();
            let d = parts.next().unwrap_or_default().to_string();
            Some(TypedRefTarget::Address { kind, pubkey, d })
        }
        "E" => Some(TypedRefTarget::EventId(comment.root_tag_value.clone())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::EventId;

    fn comment_event(id: &str, tags: Vec<Vec<&str>>) -> KernelEvent {
        KernelEvent {
            id: EventId::from(id),
            author: "commenter".to_string(),
            kind: crate::KIND_NIP22_COMMENT,
            created_at: 300,
            tags: tags
                .into_iter()
                .map(|tag| tag.into_iter().map(str::to_string).collect())
                .collect(),
            content: "nice article".to_string(),
            relay_provenance: Vec::new(),
        }
    }

    #[test]
    fn address_root_canonical_matches_the_typed_ref_target_key() {
        let mapping = nip22_root_mapping();
        let rows = mapping(&comment_event(
            "comment-1",
            vec![
                vec!["A", "30023:author:article"],
                vec!["K", "30023"],
                vec!["a", "30023:author:article"],
                vec!["k", "30023"],
            ],
        ));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].canonical_row_id, "30023:author:article");
        assert!(matches!(rows[0].payload, MappedPayload::Placeholder));
        assert_eq!(
            rows[0].context,
            vec![FeedRowContext::CommentedBy {
                author_pubkey: "commenter".to_string(),
                comment_event_id: "comment-1".to_string(),
                comment_created_at: 300,
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
    fn event_id_root_is_used_for_a_plain_note_root() {
        let mapping = nip22_root_mapping();
        let rows = mapping(&comment_event(
            "comment-1",
            vec![vec!["E", "root-event-id"], vec!["e", "root-event-id"]],
        ));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].canonical_row_id, "root-event-id");
    }

    #[test]
    fn non_comment_event_produces_no_rows() {
        let mut event = comment_event("note", Vec::new());
        event.kind = 1;
        assert!(nip22_root_mapping()(&event).is_empty());
    }
}
