use nmp_core::substrate::KernelEvent;
use nmp_nip18::try_from_kernel_event as try_from_repost_event;

use crate::RepostAttribution;

pub(super) struct RenderPayload {
    pub(super) content: String,
    pub(super) tags: Vec<Vec<String>>,
    pub(super) kind: u32,
    /// `Some` when the source event is a NIP-18 repost with an embedded
    /// inner note: the embedded note's author. `None` for ordinary notes
    /// and for e-tag-only reposts, which have no inner data to attribute.
    pub(super) author: Option<String>,
    /// `Some` when the source event is a NIP-18 repost with an embedded
    /// inner note: the embedded note's publish time. Used to build the
    /// repost attribution; the card's `created_at` stays as the outer
    /// event timestamp so the feed cursor bumps it to the top.
    note_created_at: Option<u64>,
}

impl RenderPayload {
    pub(super) fn from_event(event: &KernelEvent) -> Self {
        if let Some(repost) = try_from_repost_event(event) {
            if let Some(inner) = repost.embedded_event {
                return Self {
                    content: inner.content,
                    tags: inner.tags,
                    kind: inner.kind,
                    author: Some(inner.author),
                    note_created_at: Some(inner.created_at),
                };
            }
            // E-tag-only repost: target is not local yet, so the card has
            // no original author to attribute.
            return Self {
                content: String::new(),
                tags: Vec::new(),
                kind: nmp_nip01::KIND_SHORT_TEXT_NOTE,
                author: None,
                note_created_at: None,
            };
        }

        Self {
            content: event.content.clone(),
            tags: event.tags.clone(),
            kind: event.kind,
            author: None,
            note_created_at: None,
        }
    }

    /// Build the `reposted_by` attribution from the outer kind:6 wrapper.
    /// Ordinary notes and e-tag-only reposts return `None`.
    pub(super) fn repost_attribution(&self, outer_author: &str) -> Option<RepostAttribution> {
        let note_created_at = self.note_created_at?;
        Some(RepostAttribution {
            author_pubkey: outer_author.to_string(),
            note_created_at,
        })
    }
}
