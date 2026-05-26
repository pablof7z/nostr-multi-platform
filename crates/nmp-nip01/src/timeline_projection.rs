//! Reusable NIP-10 modular timeline projection with render-card payloads.
//!
//! `Nip10ModularTimelineView` groups event ids into blocks. Most native
//! shells also need the per-event render metadata in the same pushed snapshot,
//! so this projection owns the generic card cache beside the view state.

use std::{collections::BTreeMap, sync::Mutex, time::Instant};

use nmp_content::{tokenize_with_kind, ContentTreeWire, RenderMode, WireNode, WireNostrUriKind};
use nmp_core::substrate::{BoundedMessageMap, KernelEvent, ViewContext, MAX_PROJECTION_MESSAGES};
use nmp_core::KernelEventObserver;
use nmp_feed::FeedCard;
use nmp_nip18::try_from_kernel_event as try_from_repost_event;
use nmp_threading::TimelineBlock;
use serde::{Deserialize, Serialize};

use crate::kinds::KIND_SHORT_NOTE;
use crate::meta_timeline::{
    ModularTimelinePayload, ModularTimelineSpec, ModularTimelineState, Nip10ModularTimelineView,
};
use crate::note_relations::{NoteRelationCounts, NoteRelationIndex};
use crate::profile_display::{profile_from_event, should_replace, AuthorDisplay, ProfileDisplay};

pub use nmp_feed::{
    FeedCursor as TimelineWindowCursor, FeedPage as TimelineWindowPage,
    FeedRequest as TimelineWindowRequest, FeedWindowMetrics as TimelineWindowMetrics,
    DEFAULT_FEED_WINDOW_LIMIT as DEFAULT_TIMELINE_WINDOW_LIMIT,
    MAX_FEED_WINDOW_LIMIT as MAX_TIMELINE_WINDOW_LIMIT,
};

/// One render-ready event card surfaced through the modular timeline
/// projection. Carries raw protocol data only — pubkeys as hex,
/// timestamps as Unix seconds, display name/picture as `Option<String>`
/// (None when no kind:0 has arrived). Presentation layers own all
/// formatting decisions (aim.md §2).
///
/// For NIP-18 repost cards (a kind:6 whose target the grouper has
/// superseded) the `author_*` fields name the *original* note's author and
/// `content` carries the note's body — the kind:6 wrapper is exposed via
/// `reposted_by`. `created_at` is the *outer* event's timestamp (the repost
/// time) so the feed cursor bumps the card to the top; the underlying
/// note's publish time travels on `reposted_by.note_created_at`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TimelineEventCard {
    pub id: String,
    pub author_pubkey: String,
    pub author_display: AuthorDisplay,
    pub kind: u32,
    pub created_at: u64,
    pub content: String,
    pub content_tree: ContentTreeWire,
    /// Kernel-owned render facts for URI nodes in `content_tree`.
    ///
    /// The tree stays a protocol projection. This companion payload carries
    /// best-known, already-ingested kind:0/profile and quote-event facts so
    /// render shells can display names and embed cards without decoding,
    /// fetching, or inventing policy.
    pub content_render: ContentRenderData,
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
    /// `None` for ordinary notes; `Some` when this card was surfaced because
    /// a NIP-18 kind:6 repost superseded the original. The `author_*` fields
    /// above name the *original* note's author; this struct names who
    /// reposted it and when the original was authored (so the UI can show
    /// "<original-author> · <note-time> · ↻ reposted by <reposter>").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reposted_by: Option<RepostAttribution>,
}

/// Attribution payload for a card whose surfacing was driven by a repost.
/// Sibling to the card's primary author fields — those name the *original*
/// note's author; these fields name the *reposter*.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RepostAttribution {
    pub author_pubkey: String,
    pub author_display: AuthorDisplay,
    pub author_display_name: Option<String>,
    pub author_picture_url: Option<String>,
    /// Original note's publish time. The card's own `created_at` is the
    /// repost timestamp (used as the feed-cursor sort key so the repost
    /// bumps the note to the top); this is the timestamp the UI shows next
    /// to the original author.
    pub note_created_at: u64,
}

impl TimelineEventCard {
    fn from_event(
        event: &KernelEvent,
        profile: Option<&ProfileDisplay>,
        profiles: &BoundedMessageMap<String, ProfileDisplay>,
        cards: &BoundedMessageMap<String, TimelineEventCard>,
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
        let display_author = render_payload.author.as_deref().unwrap_or(&event.author);
        let display_profile = if render_payload.author.is_some() {
            // Embedded note's author is decoupled from the outer event's
            // author; consult the profile cache directly rather than the
            // outer event's pre-fetched `profile` argument.
            profiles.get(display_author)
        } else {
            profile
        };
        let author_display = AuthorDisplay::from_profile(display_author, display_profile);
        let author_display_name = author_display.name.clone();
        let author_picture_url = author_display.picture_url.clone();
        let content_render = content_render_for(&content_tree, profiles, cards);
        let reposted_by = render_payload.repost_attribution(&event.author, profiles);
        Self {
            id: event.id.clone(),
            author_pubkey: display_author.to_string(),
            author_display,
            kind: render_payload.kind,
            // Sort key: the outer event's `created_at`. For reposts this is
            // the repost time (so the card bumps to the top); for ordinary
            // notes it's the note's own time.
            created_at: event.created_at,
            content: render_payload.content,
            content_tree,
            content_render,
            relation_counts,
            author_display_name,
            author_picture_url,
            content_preview: content_preview(&render_payload.preview_source, 180),
            reposted_by,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ContentRenderData {
    pub profiles: BTreeMap<String, ContentProfileRenderData>,
    pub events: BTreeMap<String, ContentEventRenderData>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ContentProfileRenderData {
    pub pubkey: String,
    pub display: AuthorDisplay,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ContentEventRenderData {
    pub id: String,
    pub author_pubkey: String,
    pub author_display: AuthorDisplay,
    pub kind: u32,
    pub created_at: u64,
    pub content_preview: String,
    pub content_tree: ContentTreeWire,
}

fn content_render_for(
    tree: &ContentTreeWire,
    profiles: &BoundedMessageMap<String, ProfileDisplay>,
    cards: &BoundedMessageMap<String, TimelineEventCard>,
) -> ContentRenderData {
    let mut data = ContentRenderData::default();
    for node in &tree.nodes {
        match node {
            WireNode::Mention { uri } if uri.kind == WireNostrUriKind::Profile => {
                let pubkey = &uri.primary_id;
                data.profiles
                    .entry(pubkey.clone())
                    .or_insert_with(|| ContentProfileRenderData {
                        pubkey: pubkey.clone(),
                        display: AuthorDisplay::from_profile(pubkey, profiles.get(pubkey)),
                    });
            }
            WireNode::EventRef { uri } if uri.kind == WireNostrUriKind::Event => {
                if let Some(card) = cards.get(uri.primary_id.as_str()) {
                    data.events
                        .entry(uri.primary_id.clone())
                        .or_insert_with(|| ContentEventRenderData::from(card));
                }
            }
            _ => {}
        }
    }
    data
}

impl From<&TimelineEventCard> for ContentEventRenderData {
    fn from(card: &TimelineEventCard) -> Self {
        Self {
            id: card.id.clone(),
            author_pubkey: card.author_pubkey.clone(),
            author_display: card.author_display.clone(),
            kind: card.kind,
            created_at: card.created_at,
            content_preview: card.content_preview.clone(),
            content_tree: card.content_tree.clone(),
        }
    }
}

struct RenderPayload {
    content: String,
    preview_source: String,
    tags: Vec<Vec<String>>,
    kind: u32,
    /// `Some` when the source event is a NIP-18 repost with an embedded
    /// inner note — the embedded note's author. `None` for ordinary notes
    /// and for e-tag-only reposts (no inner data to attribute).
    author: Option<String>,
    /// `Some` when the source event is a NIP-18 repost with an embedded
    /// inner note — the embedded note's publish time. Used to build the
    /// repost attribution; the card's own `created_at` stays as the outer
    /// event's timestamp so the feed cursor bumps it to the top.
    note_created_at: Option<u64>,
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
                    author: Some(inner.author),
                    note_created_at: Some(inner.created_at),
                };
            }
            // E-tag-only repost: we don't have the inner note locally, so
            // the card has no original author to attribute. Falls back to
            // an empty placeholder card whose author + timestamp still
            // come from the outer kind:6 (caller's existing behaviour).
            return Self {
                content: String::new(),
                preview_source: String::new(),
                tags: Vec::new(),
                kind: KIND_SHORT_NOTE,
                author: None,
                note_created_at: None,
            };
        }

        Self {
            content: event.content.clone(),
            preview_source: event.content.clone(),
            tags: event.tags.clone(),
            kind: event.kind,
            author: None,
            note_created_at: None,
        }
    }

    /// Build the `reposted_by` attribution from the *outer* event (the
    /// kind:6 wrapper). Returns `None` for ordinary notes and for e-tag-only
    /// reposts (no inner note → no original-author/timestamp split to
    /// surface).
    fn repost_attribution(
        &self,
        outer_author: &str,
        profiles: &BoundedMessageMap<String, ProfileDisplay>,
    ) -> Option<RepostAttribution> {
        let note_created_at = self.note_created_at?;
        let reposter_profile = profiles.get(outer_author);
        let author_display = AuthorDisplay::from_profile(outer_author, reposter_profile);
        let author_display_name = author_display.name.clone();
        let author_picture_url = author_display.picture_url.clone();
        Some(RepostAttribution {
            author_pubkey: outer_author.to_string(),
            author_display,
            author_display_name,
            author_picture_url,
            note_created_at,
        })
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<TimelineWindowPage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<TimelineWindowMetrics>,
}

impl ModularTimelineSnapshot {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            blocks: Vec::new(),
            cards: Vec::new(),
            page: None,
            metrics: None,
        }
    }
}

impl FeedCard for TimelineEventCard {
    fn feed_created_at(&self) -> u64 {
        self.created_at
    }

    fn feed_event_refs(&self) -> Vec<String> {
        self.content_tree
            .nodes
            .iter()
            .filter_map(|node| match node {
                WireNode::EventRef { uri } if uri.kind == WireNostrUriKind::Event => {
                    Some(uri.primary_id.clone())
                }
                _ => None,
            })
            .collect()
    }
}

pub struct ModularTimelineProjection {
    inner: Mutex<Inner>,
}

struct Inner {
    state: ModularTimelineState,
    window: nmp_feed::FeedWindowState,
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
                window: nmp_feed::FeedWindowState::default(),
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
        let blocks = sorted_projection_blocks(&inner);
        ModularTimelineSnapshot {
            blocks,
            cards: inner.cards.values().cloned().collect(),
            page: None,
            metrics: None,
        }
    }

    #[must_use]
    pub fn snapshot_current_window(&self) -> ModularTimelineSnapshot {
        let make_window_start = Instant::now();
        let Ok(inner) = self.inner.lock() else {
            return ModularTimelineSnapshot::empty();
        };
        let blocks = sorted_projection_blocks(&inner);
        let (page_blocks, page) = inner.window.snapshot_blocks(&blocks, &inner.cards);
        let cards = nmp_feed::cards_for_blocks(&page_blocks, &inner.cards);
        ModularTimelineSnapshot {
            blocks: page_blocks,
            cards,
            page: Some(page),
            metrics: Some(TimelineWindowMetrics {
                make_window_us: make_window_start
                    .elapsed()
                    .as_micros()
                    .min(u64::MAX as u128) as u64,
            }),
        }
    }

    pub fn load_older_window(&self) -> bool {
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        let blocks = sorted_projection_blocks(&inner);
        let mut window = std::mem::take(&mut inner.window);
        let changed = window.load_older(&blocks, &inner.cards);
        inner.window = window;
        changed
    }

    #[must_use]
    pub fn snapshot_window(&self, request: TimelineWindowRequest) -> ModularTimelineSnapshot {
        let make_window_start = Instant::now();
        let Ok(inner) = self.inner.lock() else {
            return ModularTimelineSnapshot::empty();
        };
        let blocks = sorted_projection_blocks(&inner);
        let (page_blocks, page) = nmp_feed::page_for_request(&blocks, &inner.cards, &request);
        let cards = nmp_feed::cards_for_blocks(&page_blocks, &inner.cards);
        ModularTimelineSnapshot {
            blocks: page_blocks,
            cards,
            page: Some(page),
            metrics: Some(TimelineWindowMetrics {
                make_window_us: make_window_start
                    .elapsed()
                    .as_micros()
                    .min(u64::MAX as u128) as u64,
            }),
        }
    }
}

impl nmp_feed::FeedController for ModularTimelineProjection {
    fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot_current_window()).unwrap_or(serde_json::Value::Null)
    }

    fn load_older(&self) -> bool {
        self.load_older_window()
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
                inner.refresh_content_render_data();
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
            let card = TimelineEventCard::from_event(
                event,
                profile.as_ref(),
                &inner.profiles,
                &inner.cards,
                relation_counts,
            );
            inner.cards.insert(event.id.clone(), card);
            inner.refresh_content_render_data();
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
            // Repost cards carry a second author (the reposter) on
            // `reposted_by`. When that author's kind:0 lands later, refresh
            // the attribution so the UI shows the resolved display name.
            if let Some(attribution) = card.reposted_by.as_mut() {
                if attribution.author_pubkey == author {
                    attribution.author_display = AuthorDisplay::from_profile(author, profile);
                    attribution.author_display_name = attribution.author_display.name.clone();
                    attribution.author_picture_url = attribution.author_display.picture_url.clone();
                }
            }
        }
    }

    fn refresh_content_render_data(&mut self) {
        let profiles = &self.profiles;
        let cards = self.cards.values().cloned().collect::<Vec<_>>();
        let card_lookup = {
            let mut lookup = BTreeMap::new();
            for card in &cards {
                lookup.insert(card.id.clone(), ContentEventRenderData::from(card));
            }
            lookup
        };
        for card in self.cards.values_mut() {
            card.content_render =
                content_render_for_snapshot(&card.content_tree, profiles, &card_lookup);
        }
    }

    fn refresh_relation_counts(&mut self, event_id: &str) {
        if let Some(card) = self.cards.get_mut(event_id) {
            card.relation_counts = self.relations.counts_for(event_id);
        }
    }
}

fn content_render_for_snapshot(
    tree: &ContentTreeWire,
    profiles: &BoundedMessageMap<String, ProfileDisplay>,
    cards: &BTreeMap<String, ContentEventRenderData>,
) -> ContentRenderData {
    let mut data = ContentRenderData::default();
    for node in &tree.nodes {
        match node {
            WireNode::Mention { uri } if uri.kind == WireNostrUriKind::Profile => {
                let pubkey = &uri.primary_id;
                data.profiles
                    .entry(pubkey.clone())
                    .or_insert_with(|| ContentProfileRenderData {
                        pubkey: pubkey.clone(),
                        display: AuthorDisplay::from_profile(pubkey, profiles.get(pubkey)),
                    });
            }
            WireNode::EventRef { uri } if uri.kind == WireNostrUriKind::Event => {
                if let Some(card) = cards.get(&uri.primary_id) {
                    data.events
                        .entry(uri.primary_id.clone())
                        .or_insert_with(|| card.clone());
                }
            }
            _ => {}
        }
    }
    data
}

fn sorted_projection_blocks(inner: &Inner) -> Vec<TimelineBlock> {
    let ctx = ViewContext::default();
    let payload: ModularTimelinePayload = Nip10ModularTimelineView::snapshot(&ctx, &inner.state);
    nmp_feed::sorted_blocks(payload.blocks, &inner.cards)
}

#[cfg(test)]
#[path = "timeline_projection/tests.rs"]
mod tests;
