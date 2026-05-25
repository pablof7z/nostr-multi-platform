//! Reusable NIP-10 modular timeline projection with render-card payloads.
//!
//! `Nip10ModularTimelineView` groups event ids into blocks. Most native
//! shells also need the per-event render metadata in the same pushed snapshot,
//! so this projection owns the generic card cache beside the view state.

use std::sync::Mutex;

use nmp_content::{tokenize_with_kind, ContentTreeWire, RenderMode};
use nmp_core::display::{avatar_color_hex, display_name_initials, format_ago_secs, short_hex};
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
    profile_from_event, should_replace, AuthorDisplay, AuthorDisplaySource, ProfileDisplay,
};

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
    /// V-27 thin-shell: relative "X ago" string for `created_at`. Computed in
    /// Rust at snapshot construction so the host shell never reaches for a
    /// clock. Delegates to [`nmp_core::display::format_ago_secs`] (V-33) —
    /// the canonical `Xs/Xm/Xh/Xd ago` dialect every NMP surface speaks.
    pub created_at_display: String,
    /// Two-character uppercase initials for avatar renderers.
    pub author_avatar_initials: String,
    /// Deterministic 6-hex avatar background colour (uppercase, no `#`
    /// prefix). Delegates to [`nmp_core::display::avatar_color_hex`] so the
    /// same author renders with the same tint across NMP surfaces.
    pub author_avatar_color: String,
    /// Abbreviated hex pubkey (`<first 8>…<last 8>`) for secondary identity
    /// captions. Delegates to [`nmp_core::display::short_hex`] so every
    /// consumer speaks the same display dialect.
    pub author_pubkey_short: String,
    /// Flat mirror of `author_display.name` for renderers that want a simple
    /// display-name field without decoding the nested `AuthorDisplay` object.
    pub author_display_name: String,
    /// Abbreviated event id (`<first 8>…<last 8>`) for host surfaces that want
    /// a compact monospaced reference to this event.
    pub short_id: String,
    /// Author's profile picture URL. Mirrors `AuthorDisplay.picture_url`:
    /// kind:0 `picture` when available, otherwise the canonical
    /// `identicon:<first16-hex>` placeholder.
    pub author_picture_url: String,
    /// First 180 Unicode scalars of render content, no ellipsis appended.
    /// Scalar-based (`chars()`) rather than grapheme-cluster-based; for Nostr
    /// text this is indistinguishable in practice.
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
        // V-32 thin-shell: reuse the picture URL `AuthorDisplay::from_profile`
        // already resolved (kind:0 `picture` field or `picture_placeholder`
        // fallback). One source of truth for avatar resolution; do NOT
        // recompute the identicon prefix here.
        let author_picture_url = author_display.picture_url.clone();
        // V-34 thin-shell: initials from the display name, not raw hex chars.
        // Extracted before the struct literal moves `author_display`. Matches
        // `TimelineItem.author_avatar_initials`: ".." until Kind0 lands.
        let author_avatar_initials = match author_display.source {
            AuthorDisplaySource::Kind0 => display_name_initials(&author_display.name),
            AuthorDisplaySource::Npub => "..".to_string(),
        };
        Self {
            id: event.id.clone(),
            author_pubkey: event.author.clone(),
            author_display,
            kind: event.kind,
            created_at: event.created_at,
            content: render_payload.content,
            content_tree,
            relation_counts,
            created_at_display: format_ago_secs(now_unix_secs(), event.created_at),
            author_avatar_initials,
            author_avatar_color: avatar_color_hex(&event.author),
            author_pubkey_short: short_hex(&event.author),
            author_display_name,
            // V-28 thin-shell: same `<first 8>…<last 8>` abbreviation
            // algorithm `author_pubkey_short` uses — `pubkey_display` is
            // generic over any hex string, so we reuse it on `event.id`.
            short_id: short_hex(&event.id),
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

// ── V-27 thin-shell display helpers ───────────────────────────────────────
//
// All cross-surface display helpers are imported from [`nmp_core::display`]
// (V-33): `format_ago_secs`, `avatar_color_hex`, `display_name_initials`,
// and now `short_hex` (`<first-8>…<last-8>` for raw hex IDs).

fn now_unix_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
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
