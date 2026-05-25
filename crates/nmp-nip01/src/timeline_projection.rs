//! Reusable NIP-10 modular timeline projection with render-card payloads.
//!
//! `Nip10ModularTimelineView` groups event ids into blocks. Most native
//! shells also need the per-event render metadata in the same pushed snapshot,
//! so this projection owns the generic card cache beside the view state.

use std::sync::Mutex;

use nmp_content::{tokenize_with_kind, ContentTreeWire, RenderMode};
use nmp_core::substrate::{BoundedMessageMap, KernelEvent, ViewContext, MAX_PROJECTION_MESSAGES};
use nmp_core::KernelEventObserver;
use nmp_nip18::try_from_kernel_event as try_from_repost_event;
use nmp_threading::TimelineBlock;
use serde::{Deserialize, Serialize};

use crate::kinds::KIND_SHORT_NOTE;
use crate::meta_timeline::{
    ModularTimelinePayload, ModularTimelineSpec, ModularTimelineState, Nip10ModularTimelineView,
};
use crate::note_relations::{NoteRelationCounts, NoteRelationIndex};
use crate::profile_display::{
    profile_from_event, should_replace, AuthorDisplay, ProfileDisplay,
};

/// One render-ready event card surfaced through the modular timeline
/// projection. Carries raw protocol data only — pubkeys as hex,
/// timestamps as Unix seconds, display name/picture as `Option<String>`
/// (None when no kind:0 has arrived). Presentation layers own all
/// formatting decisions (aim.md §2).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TimelineEventCard {
    pub id: String,
    pub author_pubkey: String,
    pub author_display: AuthorDisplay,
    pub kind: u32,
    pub created_at: u64,
    pub content: String,
    pub content_tree: ContentTreeWire,
    pub relation_counts: NoteRelationCounts,
    /// Flat mirror of `author_display.name` for renderers that want a
    /// simple display-name field without decoding the nested
    /// `AuthorDisplay` object. `None` when no kind:0 has arrived yet for
    /// this author — presentation layer falls back to formatting
    /// `author_pubkey` itself.
    pub author_display_name: Option<String>,
    /// Author's profile picture URL from kind:0. `None` when no kind:0
    /// has arrived, or the kind:0 omits `picture` — presentation layer
    /// chooses a placeholder/identicon strategy.
    pub author_picture_url: Option<String>,
    /// First 180 Unicode scalars of render content, no ellipsis appended.
    /// Scalar-based (`chars()`) rather than grapheme-cluster-based; for
    /// Nostr text this is indistinguishable in practice.
    pub content_preview: String,
}

impl TimelineEventCard {
    fn from_event(
        event: &KernelEvent,
        profile: Option<&ProfileDisplay>,
        relation_counts: NoteRelationCounts,
    ) -> Self {
        let render_payload = RenderPayload::from_event(event);
        let content_tree = tokenize_with_kind(
            &render_payload.content,
            &render_payload.tags,
            RenderMode::Auto,
            render_payload.kind,
        )
        .to_wire();
        let author_display = AuthorDisplay::from_profile(&event.author, profile);
        let author_display_name = author_display.name.clone();
        let author_picture_url = author_display.picture_url.clone();
        Self {
            id: event.id.clone(),
            author_pubkey: event.author.clone(),
            author_display,
            kind: event.kind,
            created_at: event.created_at,
            content: render_payload.content,
            content_tree,
            relation_counts,
            author_display_name,
            author_picture_url,
            content_preview: content_preview(&render_payload.preview_source, 180),
        }
    }
}

struct RenderPayload {
    content: String,
    preview_source: String,
    tags: Vec<Vec<String>>,
    kind: u32,
}

impl RenderPayload {
    fn from_event(event: &KernelEvent) -> Self {
        if let Some(repost) = try_from_repost_event(event) {
            if let Some(inner) = repost.embedded_event {
                return Self {
                    preview_source: inner.content.clone(),
                    content: inner.content,
                    tags: inner.tags,
                    kind: inner.kind,
                };
            }
            return Self {
                content: String::new(),
                preview_source: String::new(),
                tags: Vec::new(),
                kind: KIND_SHORT_NOTE,
            };
        }

        Self {
            content: event.content.clone(),
            preview_source: event.content.clone(),
            tags: event.tags.clone(),
            kind: event.kind,
        }
    }
}

fn has_render_card(event: &KernelEvent) -> bool {
    crate::try_from_kernel_event(event).is_some() || try_from_repost_event(event).is_some()
}

/// First `n` Unicode scalars of `content`, no ellipsis.
fn content_preview(content: &str, n: usize) -> String {
    content.chars().take(n).collect()
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ModularTimelineSnapshot {
    pub blocks: Vec<TimelineBlock>,
    pub cards: Vec<TimelineEventCard>,
}

impl ModularTimelineSnapshot {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            blocks: Vec::new(),
            cards: Vec::new(),
        }
    }
}

pub struct ModularTimelineProjection {
    inner: Mutex<Inner>,
}

struct Inner {
    state: ModularTimelineState,
    cards: BoundedMessageMap<String, TimelineEventCard>,
    profiles: BoundedMessageMap<String, ProfileDisplay>,
    relations: NoteRelationIndex,
}

impl ModularTimelineProjection {
    #[must_use]
    pub fn new(spec: &ModularTimelineSpec) -> Self {
        let ctx = ViewContext::default();
        let (state, _payload) = Nip10ModularTimelineView::open(&ctx, spec);
        Self {
            inner: Mutex::new(Inner {
                state,
                cards: BoundedMessageMap::new(MAX_PROJECTION_MESSAGES),
                profiles: BoundedMessageMap::new(MAX_PROJECTION_MESSAGES),
                relations: NoteRelationIndex::default(),
            }),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> ModularTimelineSnapshot {
        let Ok(inner) = self.inner.lock() else {
            return ModularTimelineSnapshot::empty();
        };
        let ctx = ViewContext::default();
        let payload: ModularTimelinePayload =
            Nip10ModularTimelineView::snapshot(&ctx, &inner.state);
        ModularTimelineSnapshot {
            blocks: payload.blocks,
            cards: inner.cards.values().cloned().collect(),
        }
    }
}

impl KernelEventObserver for ModularTimelineProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let ctx = ViewContext::default();
        if let Some(profile) = profile_from_event(event) {
            if should_replace(inner.profiles.get(&event.author), &profile) {
                inner.profiles.insert(event.author.clone(), profile);
                inner.refresh_author_cards(&event.author);
            }
            return;
        }
        let changed_relation_targets = inner.relations.ingest(event);
        for target in changed_relation_targets {
            inner.refresh_relation_counts(&target);
        }
        if has_render_card(event) {
            let profile = inner.profiles.get(&event.author).cloned();
            let relation_counts = inner.relations.counts_for(&event.id);
            inner.cards.insert(
                event.id.clone(),
                TimelineEventCard::from_event(event, profile.as_ref(), relation_counts),
            );
        }
        let _ = Nip10ModularTimelineView::on_event_inserted(&ctx, &mut inner.state, event);
        // delta unused — projection takes snapshots directly
    }
}

impl Inner {
    fn refresh_author_cards(&mut self, author: &str) {
        let profile = self.profiles.get(author);
        for card in self.cards.values_mut() {
            if card.author_pubkey == author {
                card.author_display = AuthorDisplay::from_profile(author, profile);
                // V-27 thin-shell: keep the flat mirror in sync so host
                // renderers see the profile-loaded display name on the next
                // snapshot.
                card.author_display_name = card.author_display.name.clone();
                // V-32 thin-shell: same rationale for the picture URL —
                // when a kind:0 arrives after the note, the card's
                // identicon placeholder must be replaced by the parsed
                // profile picture on the next snapshot.
                card.author_picture_url = card.author_display.picture_url.clone();
            }
        }
    }

    fn refresh_relation_counts(&mut self, event_id: &str) {
        if let Some(card) = self.cards.get_mut(event_id) {
            card.relation_counts = self.relations.counts_for(event_id);
        }
    }
}

#[cfg(test)]
#[path = "timeline_projection/tests.rs"]
mod tests;
