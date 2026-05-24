//! Reusable NIP-10 modular timeline projection with render-card payloads.
//!
//! `Nip10ModularTimelineView` groups event ids into blocks. Most native
//! shells also need the per-event render metadata in the same pushed snapshot,
//! so this projection owns the generic card cache beside the view state.

use std::sync::Mutex;

use nmp_content::{tokenize_with_kind, ContentTreeWire, RenderMode};
use nmp_core::display::{avatar_color_hex, display_name_initials, format_ago_secs};
use nmp_core::substrate::{BoundedMessageMap, KernelEvent, MAX_PROJECTION_MESSAGES, ViewContext};
use nmp_core::KernelEventObserver;
use nmp_threading::TimelineBlock;
use serde::{Deserialize, Serialize};

use crate::meta_timeline::{
    ModularTimelinePayload, ModularTimelineSpec, ModularTimelineState, Nip10ModularTimelineView,
};
use crate::note_relations::{NoteRelationCounts, NoteRelationIndex};
use crate::profile_display::{
    profile_from_event, should_replace, AuthorDisplay, AuthorDisplaySource, ProfileDisplay,
};
use crate::try_from_kernel_event;

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
    /// V-27 thin-shell: two-char uppercase initials for the avatar tile,
    /// derived from `author_pubkey`. Mirrors the `pubkey_initials` helper
    /// deleted from `ModularBlockView.swift`.
    pub author_avatar_initials: String,
    /// V-27 thin-shell: deterministic 6-hex avatar background colour
    /// (uppercase, no `#` prefix). Delegates to the canonical
    /// [`nmp_core::display::avatar_color_hex`] (V-33) so the same author
    /// renders with the same tint across every NMP surface (DMs, NIP-29
    /// group chat, the modular timeline, the Accounts toolbar, Marmot rows).
    pub author_avatar_color: String,
    /// V-27 thin-shell: abbreviated hex pubkey for the Twitter-style
    /// secondary-identifier slot (the "@handle" caption beneath the display
    /// name). `<first 8>…<last 8>` — same algorithm as
    /// `nmp_nip29::projection::group_chat::pubkey_display` so DMs, NIP-29
    /// rows, and the modular timeline speak the same dialect. Replaces the
    /// `displayPubkey` helper deleted from `ModularBlockView.swift` (the
    /// old Swift helper used `<first 6>…<last 4>`; aligning to the
    /// cross-surface algorithm shifts that abbreviation by two characters).
    pub author_pubkey_short: String,
    /// V-27 thin-shell: flat mirror of `author_display.name` so Swift can
    /// bind a single string without decoding the nested `AuthorDisplay`
    /// struct. Synthetic-from-card rows in `ModularBlockView.swift` use
    /// this as the display-name fallback when no `TimelineItem` is loaded.
    pub author_display_name: String,
    /// V-28 thin-shell: abbreviated event id (`<first 8>…<last 8>`) used by
    /// the synthetic `TimelineItem` builder in `ModularBlockView.swift` to
    /// populate `TimelineItem.short_id` (and by any host surface that wants a
    /// compact monospaced reference to this event). Mirrors the
    /// `author_pubkey_short` field above — same `pubkey_display` algorithm
    /// works on any hex string. Required because Swift must NEVER slice the
    /// raw 64-char `id` to compute an abbreviation (V-28, aim.md §6.9).
    pub short_id: String,
    /// V-32 thin-shell: author's profile picture URL. Mirrors
    /// `AuthorDisplay.picture_url` — when a kind:0 profile is loaded this is
    /// the parsed `picture` URL; otherwise it is the
    /// `identicon:<first16-hex>` placeholder produced by
    /// `nmp_core::substrate::picture_placeholder`. The synthetic
    /// `TimelineItem` builder in `ModularBlockView.swift` binds this directly
    /// instead of recomputing the placeholder in Swift (which used a shorter
    /// `<first 8>` prefix — deliberate alignment to the cross-surface
    /// `picture_placeholder` algorithm, NOT a regression).
    pub author_picture_url: String,
    /// V-32 thin-shell: first 180 Unicode scalars of `content`, used by the
    /// synthetic `TimelineItem` builder in `ModularBlockView.swift` so Swift
    /// never calls `String(card.content.prefix(180))`. No ellipsis appended —
    /// matches the prior Swift call-site exactly. Scalar-based (`chars()`)
    /// rather than grapheme-cluster-based; for Nostr text this is
    /// indistinguishable in practice.
    pub content_preview: String,
}

impl TimelineEventCard {
    fn from_event(
        event: &KernelEvent,
        profile: Option<&ProfileDisplay>,
        relation_counts: NoteRelationCounts,
    ) -> Self {
        let content_tree =
            tokenize_with_kind(&event.content, &event.tags, RenderMode::Auto, event.kind).to_wire();
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
            content: event.content.clone(),
            content_tree,
            relation_counts,
            created_at_display: format_ago_secs(now_unix_secs(), event.created_at),
            author_avatar_initials,
            author_avatar_color: avatar_color_hex(&event.author),
            author_pubkey_short: pubkey_display(&event.author),
            author_display_name,
            // V-28 thin-shell: same `<first 8>…<last 8>` abbreviation
            // algorithm `author_pubkey_short` uses — `pubkey_display` is
            // generic over any hex string, so we reuse it on `event.id`.
            short_id: pubkey_display(&event.id),
            author_picture_url,
            // V-32 thin-shell: scalar-based truncation matches the prior
            // Swift `String(card.content.prefix(180))` call-site verbatim.
            content_preview: content_preview(&event.content, 180),
        }
    }
}

// ── V-27 thin-shell display helpers ───────────────────────────────────────
//
// `format_ago_secs`, `avatar_color_hex`, and `display_name_initials` are
// imported from [`nmp_core::display`] — the canonical home for cross-surface
// formatting primitives (V-33). One local helper remains:
// - `pubkey_display(hex)` — `<first-8>…<last-8>` for raw hex IDs (event.id
//   and event.author). Distinct from the bech32-aware `short_npub`.

fn now_unix_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn pubkey_display(pubkey_hex: &str) -> String {
    if pubkey_hex.len() < 16 {
        return pubkey_hex.to_string();
    }
    format!(
        "{}…{}",
        &pubkey_hex[..8],
        &pubkey_hex[pubkey_hex.len() - 8..]
    )
}

/// V-32 thin-shell: first `n` Unicode scalars of `content`, no ellipsis.
/// Replaces the Swift `String(card.content.prefix(180))` call-site in
/// `ModularBlockView.swift`'s synthetic-item builder. Scalar-based (`chars()`)
/// rather than grapheme-cluster-based — for the short Nostr-text inputs this
/// preview targets the two are indistinguishable in practice.
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
        if try_from_kernel_event(event).is_some() {
            let profile = inner.profiles.get(&event.author).cloned();
            let relation_counts = inner.relations.counts_for(&event.id);
            inner.cards.insert(
                event.id.clone(),
                TimelineEventCard::from_event(event, profile.as_ref(), relation_counts),
            );
        }
        let _ = Nip10ModularTimelineView::on_event_inserted(&ctx, &mut inner.state, event); // delta unused — projection takes snapshots directly
    }
}

impl Inner {
    fn refresh_author_cards(&mut self, author: &str) {
        let profile = self.profiles.get(author);
        for card in self.cards.values_mut() {
            if card.author_pubkey == author {
                card.author_display = AuthorDisplay::from_profile(author, profile);
                // V-27 thin-shell: keep the flat mirror in sync so Swift
                // sees the profile-loaded display name on the next snapshot.
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
mod tests {
    use super::*;
    use nmp_content::{WireNode, WireNostrUriKind};
    use nmp_core::nip19::encode_npub;
    use nmp_threading::{ModulePolicy, TimelineBlock};
    use std::sync::Arc;

    fn spec() -> ModularTimelineSpec {
        ModularTimelineSpec {
            viewer: "me".into(),
            kinds: vec![],
            authors: None,
            policy: ModulePolicy::default(),
        }
    }

    fn note(id: &str, ts: u64, tags: Vec<Vec<String>>) -> KernelEvent {
        note_with_content(id, ts, tags, id)
    }

    fn note_with_content(id: &str, ts: u64, tags: Vec<Vec<String>>, content: &str) -> KernelEvent {
        KernelEvent {
            id: id.into(),
            author: "auth".into(),
            kind: 1,
            created_at: ts,
            tags,
            content: content.into(),
        }
    }

    fn reply_to(id: &str, ts: u64, root: &str, parent: &str) -> KernelEvent {
        note(
            id,
            ts,
            vec![
                vec!["e".into(), root.into(), "".into(), "root".into()],
                vec!["e".into(), parent.into(), "".into(), "reply".into()],
            ],
        )
    }

    #[test]
    fn empty_open_yields_empty_snapshot() {
        let proj = ModularTimelineProjection::new(&spec());
        let snap = proj.snapshot();
        assert!(snap.blocks.is_empty());
        assert!(snap.cards.is_empty());
    }

    #[test]
    fn root_plus_reply_collapses_into_one_module() {
        let proj = ModularTimelineProjection::new(&spec());
        proj.on_kernel_event(&note("R", 1, vec![]));
        proj.on_kernel_event(&reply_to("C", 2, "R", "R"));
        let snap = proj.snapshot();
        assert_eq!(snap.blocks.len(), 1);
        match &snap.blocks[0] {
            TimelineBlock::Module { events, .. } => {
                assert_eq!(events, &vec!["R".to_string(), "C".to_string()]);
            }
            other => panic!("expected Module, got {other:?}"),
        }
        assert_eq!(snap.cards.len(), 2);
    }

    #[test]
    fn standalone_event_becomes_standalone_block() {
        let proj = ModularTimelineProjection::new(&spec());
        proj.on_kernel_event(&note("S", 1, vec![]));
        let snap = proj.snapshot();
        assert_eq!(snap.blocks.len(), 1);
        assert!(matches!(snap.blocks[0], TimelineBlock::Standalone(_)));
    }

    #[test]
    fn cards_include_content_tree_wire_for_mentions() {
        const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
        let mention = format!("nostr:{}", encode_npub(PK).expect("fixture npub encodes"));
        let proj = ModularTimelineProjection::new(&spec());
        proj.on_kernel_event(&note_with_content(
            "S",
            1,
            vec![],
            &format!("hello {mention} #nostr"),
        ));

        let snap = proj.snapshot();
        let card = snap
            .cards
            .iter()
            .find(|c| c.id == "S")
            .expect("card exists");
        assert!(card.content_tree.nodes.iter().any(|node| {
            matches!(
                node,
                WireNode::Mention { uri }
                    if uri.kind == WireNostrUriKind::Profile && uri.primary_id == PK
            )
        }));
    }

    #[test]
    fn observer_trait_object_drives_grouper() {
        let proj: Arc<dyn KernelEventObserver> = Arc::new(ModularTimelineProjection::new(&spec()));
        proj.on_kernel_event(&note("X", 1, vec![]));
    }

    // ── V-27 thin-shell display-field tests ──────────────────────────────

    #[test]
    fn card_carries_v27_display_fields_for_ingested_event() {
        const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
        let event = KernelEvent {
            id: "E".into(),
            author: PK.into(),
            kind: 1,
            created_at: 1,
            tags: vec![],
            content: "hello".into(),
        };
        let proj = ModularTimelineProjection::new(&spec());
        proj.on_kernel_event(&event);
        let snap = proj.snapshot();
        let card = snap
            .cards
            .iter()
            .find(|c| c.id == "E")
            .expect("card exists");

        // initials: ".." placeholder until Kind0 lands (V-34)
        assert_eq!(card.author_avatar_initials, "..");
        // colour: deterministic djb2 hex, 6 uppercase hex chars, no `#`
        assert_eq!(card.author_avatar_color.len(), 6);
        assert!(card
            .author_avatar_color
            .chars()
            .all(|c| c.is_ascii_hexdigit() && (c.is_ascii_digit() || c.is_ascii_uppercase())));
        // pubkey short: 8…8 with ellipsis when hex is long
        assert_eq!(card.author_pubkey_short, "3bf0c63f…aefa459d");
        // created_at_display: a pinned old timestamp resolves to "Xd ago"
        // (test runs well after 1970 so the bucket is `d`).
        assert!(
            card.created_at_display.ends_with(" ago"),
            "expected `Xd ago`, got {}",
            card.created_at_display
        );
        // flat display-name mirror equals nested AuthorDisplay.name.
        assert_eq!(card.author_display_name, card.author_display.name);
        assert!(!card.author_display_name.is_empty());
    }

    // The canonical pinned djb2 vector and exhaustive `format_ago_secs`
    // bucket coverage live in `nmp_core::display::tests` (V-33). The
    // `card_carries_v27_display_fields_for_ingested_event` test above pins
    // the call-site result (`PK = "3bf0…"` → `card.author_avatar_color`)
    // so a drift in the canonical helper still surfaces at this layer.

    #[test]
    fn display_name_initials_word_based() {
        // word-based: first char of each word, uppercase (canonical algorithm)
        assert_eq!(display_name_initials("Alice Smith"), "AS");
        assert_eq!(display_name_initials("alice bob"), "AB");
        assert_eq!(display_name_initials("bob"), "B.");
        assert_eq!(display_name_initials("a"), "A.");
        assert_eq!(display_name_initials(""), "..");
    }

    #[test]
    fn pubkey_display_short_inputs_returned_unchanged() {
        assert_eq!(pubkey_display(""), "");
        assert_eq!(pubkey_display("abcd"), "abcd");
        // boundary: exactly 16 chars triggers abbreviation
        assert_eq!(pubkey_display("0123456789abcdef"), "01234567…89abcdef");
    }

    #[test]
    fn refresh_author_cards_updates_v27_display_name_when_kind0_arrives_later() {
        const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
        let proj = ModularTimelineProjection::new(&spec());
        // First the note arrives with no profile loaded — display_name is the npub fallback.
        let note_event = KernelEvent {
            id: "E".into(),
            author: PK.into(),
            kind: 1,
            created_at: 1,
            tags: vec![],
            content: "hi".into(),
        };
        proj.on_kernel_event(&note_event);
        let pre = proj
            .snapshot()
            .cards
            .into_iter()
            .find(|c| c.id == "E")
            .expect("card");
        assert!(pre.author_display_name.starts_with("npub1"));

        // Then a kind:0 arrives — the flat mirror must update.
        let profile_event = KernelEvent {
            id: "P".into(),
            author: PK.into(),
            kind: 0,
            created_at: 2,
            tags: vec![],
            content: r#"{"display_name":"Alice"}"#.into(),
        };
        proj.on_kernel_event(&profile_event);
        let post = proj
            .snapshot()
            .cards
            .into_iter()
            .find(|c| c.id == "E")
            .expect("card");
        assert_eq!(post.author_display_name, "Alice");
        assert_eq!(post.author_display.name, "Alice");
    }

    // ── V-32 thin-shell tests ───────────────────────────────────────────

    #[test]
    fn card_carries_v32_picture_url_and_content_preview() {
        const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
        let event = KernelEvent {
            id: "E".into(),
            author: PK.into(),
            kind: 1,
            created_at: 1,
            tags: vec![],
            content: "hello world".into(),
        };
        let proj = ModularTimelineProjection::new(&spec());
        proj.on_kernel_event(&event);
        let snap = proj.snapshot();
        let card = snap
            .cards
            .iter()
            .find(|c| c.id == "E")
            .expect("card exists");

        // No profile loaded yet → identicon placeholder from nmp-core
        // (`picture_placeholder` uses the first 16 hex chars, NOT 8 —
        // deliberate alignment with the cross-surface placeholder).
        assert_eq!(card.author_picture_url, "identicon:3bf0c63fcb934634");
        // Field must equal the nested `AuthorDisplay.picture_url` —
        // single source of truth.
        assert_eq!(card.author_picture_url, card.author_display.picture_url);

        // content_preview: short content passes through unchanged, no ellipsis.
        assert_eq!(card.content_preview, "hello world");
    }

    #[test]
    fn content_preview_truncates_at_180_scalars_without_ellipsis() {
        // 200-char ASCII body → preview is the first 180 chars, no `…`.
        let body = "a".repeat(200);
        let expected = "a".repeat(180);
        let event = KernelEvent {
            id: "L".into(),
            author: "a".repeat(64),
            kind: 1,
            created_at: 1,
            tags: vec![],
            content: body,
        };
        let proj = ModularTimelineProjection::new(&spec());
        proj.on_kernel_event(&event);
        let card = proj
            .snapshot()
            .cards
            .into_iter()
            .find(|c| c.id == "L")
            .expect("card");
        assert_eq!(card.content_preview.len(), 180);
        assert_eq!(card.content_preview, expected);
        assert!(!card.content_preview.ends_with('…'));
    }

    #[test]
    fn refresh_author_cards_updates_v32_picture_url_when_kind0_arrives_later() {
        const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
        let proj = ModularTimelineProjection::new(&spec());
        // Note arrives first → identicon placeholder.
        proj.on_kernel_event(&KernelEvent {
            id: "E".into(),
            author: PK.into(),
            kind: 1,
            created_at: 1,
            tags: vec![],
            content: "hi".into(),
        });
        let pre = proj
            .snapshot()
            .cards
            .into_iter()
            .find(|c| c.id == "E")
            .expect("card");
        assert!(pre.author_picture_url.starts_with("identicon:"));

        // Kind:0 with a real picture URL arrives — the flat mirror must update.
        proj.on_kernel_event(&KernelEvent {
            id: "P".into(),
            author: PK.into(),
            kind: 0,
            created_at: 2,
            tags: vec![],
            content: r#"{"display_name":"Alice","picture":"https://example.com/a.png"}"#.into(),
        });
        let post = proj
            .snapshot()
            .cards
            .into_iter()
            .find(|c| c.id == "E")
            .expect("card");
        assert_eq!(post.author_picture_url, "https://example.com/a.png");
        assert_eq!(post.author_picture_url, post.author_display.picture_url);
    }
}
