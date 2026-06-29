//! The cross-protocol [`NoteRelationClassifier`] — the social-relation
//! aggregation lifted out of `nmp-nip01` (#1728).
//!
//! `nmp-nip01` counts its own kind:1 NIP-10 replies natively and exposes the
//! [`NoteRelationClassifier`] seam for everything else. This module supplies the
//! concrete classifier that recognises the v1 default cross-protocol relation
//! sources: reactions (NIP-25 kind:7), reposts (NIP-18 kind:6), zaps (NIP-57
//! kind:9735), and comments (NIP-22 kind:1111). The base note crate carries no
//! dependency on those NIP crates.

use std::sync::Arc;

use nmp_core::substrate::KernelEvent;
use nmp_nip01::{ClassifiedRelation, NoteRelationClassifier, RelationKind};

/// Production [`NoteRelationClassifier`]: classifies reactions, reposts, zaps,
/// and comments onto the note they reference. kind:1 replies are NOT classified
/// here — `nmp-nip01` owns its own reply detection.
pub struct DefaultNoteRelationClassifier;

impl NoteRelationClassifier for DefaultNoteRelationClassifier {
    fn classify(&self, event: &KernelEvent) -> Option<ClassifiedRelation> {
        // NIP-22 kind:1111 comment — counted against its UPPERCASE root scope
        // target (the artifact the thread hangs off), so an event/article/
        // external root surfaces a comment count.
        if let Some(comment) = nmp_nip22::try_from_kernel_event(event) {
            if comment.root_tag_value.is_empty() {
                return None;
            }
            return Some(ClassifiedRelation {
                target: comment.root_tag_value,
                kind: RelationKind::Comment,
            });
        }
        // NIP-18 kind:6/16 reposts — addressable targets count against their
        // canonical `kind:pubkey:d` coordinate; event-only targets count by id.
        if nmp_nip18::is_repost_kind(event.kind) {
            return nmp_nip18::try_from_kernel_event(event)
                .and_then(|repost| {
                    repost
                        .target_address
                        .map(|coord| coord.to_wire())
                        .or(repost.target_event_id)
                })
                .map(|target| ClassifiedRelation {
                    target,
                    kind: RelationKind::Repost,
                });
        }
        // NIP-57 kind:9735 zap receipt — counted against the zapped address
        // coordinate when present, otherwise the zapped event id.
        if event.kind == nmp_nip57::KIND_ZAP_RECEIPT {
            return nmp_nip57::try_from_kernel_event(event)
                .and_then(|zap| zap.zapped_address.or(zap.zapped_event_id))
                .map(|target| ClassifiedRelation {
                    target,
                    kind: RelationKind::Zap,
                });
        }
        // NIP-25 kind:7 reaction — counted against its first `e` tag.
        if event.kind == nmp_kinds::KIND_REACTION {
            return first_event_tag(&event.tags).map(|target| ClassifiedRelation {
                target,
                kind: RelationKind::Reaction,
            });
        }
        None
    }
}

/// A fresh `Arc<dyn NoteRelationClassifier>` backed by
/// [`DefaultNoteRelationClassifier`], ready to inject into
/// `nmp_nip01::NoteRelationIndex::new` or
/// `nmp_nip01::ModularTimelineProjection::with_relation_classifier`.
#[must_use]
pub fn default_note_relation_classifier() -> Arc<dyn NoteRelationClassifier> {
    Arc::new(DefaultNoteRelationClassifier)
}

fn first_event_tag(tags: &[Vec<String>]) -> Option<String> {
    tags.iter().find_map(|tag| {
        if tag.first().is_some_and(|name| name == "e") {
            tag.get(1).filter(|id| !id.is_empty()).cloned()
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_nip01::NoteRelationIndex;

    fn kernel_event(id: &str, kind: u32, tags: Vec<Vec<String>>) -> KernelEvent {
        KernelEvent {
            id: id.to_string(),
            author: "a".repeat(64),
            kind,
            created_at: 1,
            tags,
            content: String::new(),
            relay_provenance: Vec::new(),
        }
    }

    #[test]
    fn classifies_kind7_reaction_against_first_e_tag() {
        let react = kernel_event(
            &"c".repeat(64),
            7,
            vec![vec!["e".to_string(), "root".to_string()]],
        );
        let classified = DefaultNoteRelationClassifier.classify(&react);
        assert_eq!(
            classified,
            Some(ClassifiedRelation {
                target: "root".to_string(),
                kind: RelationKind::Reaction,
            })
        );
    }

    #[test]
    fn does_not_classify_kind1_replies_those_are_native() {
        // kind:1 is NIP-01's own domain — the classifier must not claim it.
        let note = kernel_event(
            &"d".repeat(64),
            1,
            vec![vec!["e".to_string(), "root".to_string()]],
        );
        assert!(DefaultNoteRelationClassifier.classify(&note).is_none());
    }

    #[test]
    fn index_with_default_classifier_counts_nip22_comments_against_root() {
        let mut index = NoteRelationIndex::new(Some(default_note_relation_classifier()));
        let root = "r".repeat(64);
        let comment = kernel_event(
            &"c".repeat(64),
            nmp_nip22::KIND_NIP22_COMMENT,
            vec![
                vec!["E".to_string(), root.clone()],
                vec!["K".to_string(), "11".to_string()],
                vec!["e".to_string(), root.clone()],
                vec!["k".to_string(), "11".to_string()],
            ],
        );

        assert_eq!(index.ingest(&comment), vec![root.clone()]);
        let counts = index.counts_for(&root);
        assert_eq!(
            counts.comments,
            nmp_nip01::RelationCount::Known { count: 1 }
        );
        // A comment is not also tallied as a kind:1 reply.
        assert_eq!(counts.replies, nmp_nip01::RelationCount::Known { count: 0 });
    }

    #[test]
    fn index_with_default_classifier_counts_kind7_reactions() {
        let mut index = NoteRelationIndex::new(Some(default_note_relation_classifier()));
        let react = kernel_event(
            &"c".repeat(64),
            7,
            vec![vec!["e".to_string(), "root".to_string()]],
        );
        assert_eq!(index.ingest(&react), vec!["root".to_string()]);
        assert_eq!(
            index.counts_for("root").reactions,
            nmp_nip01::RelationCount::Known { count: 1 }
        );
    }

    #[test]
    fn index_with_default_classifier_counts_generic_reposts_against_address() {
        let mut index = NoteRelationIndex::new(Some(default_note_relation_classifier()));
        let address = format!("30023:{}:article", "b".repeat(64));
        let repost = kernel_event(
            &"c".repeat(64),
            nmp_nip18::KIND_GENERIC_REPOST,
            vec![
                vec!["a".to_string(), address.clone()],
                vec!["k".to_string(), "30023".to_string()],
            ],
        );
        assert_eq!(index.ingest(&repost), vec![address.clone()]);
        assert_eq!(
            index.counts_for(&address).reposts,
            nmp_nip01::RelationCount::Known { count: 1 }
        );
    }

    #[test]
    fn index_with_default_classifier_counts_zaps_against_address() {
        let mut index = NoteRelationIndex::new(Some(default_note_relation_classifier()));
        let address = format!("30023:{}:article", "b".repeat(64));
        let zap = kernel_event(
            &"c".repeat(64),
            nmp_nip57::KIND_ZAP_RECEIPT,
            vec![
                vec!["p".to_string(), "recipient".to_string()],
                vec!["a".to_string(), address.clone()],
            ],
        );
        assert_eq!(index.ingest(&zap), vec![address.clone()]);
        assert_eq!(
            index.counts_for(&address).zaps,
            nmp_nip01::RelationCount::Known { count: 1 }
        );
    }
}
