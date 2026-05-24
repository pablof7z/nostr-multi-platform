//! Reusable NIP-10 modular timeline projection with render-card payloads.
//!
//! `Nip10ModularTimelineView` groups event ids into blocks. Most native
//! shells also need the per-event render metadata in the same pushed snapshot,
//! so this projection owns the generic card cache beside the view state.

use std::sync::Mutex;

use nmp_content::{tokenize_with_kind, ContentTreeWire, RenderMode};
use nmp_core::substrate::{BoundedMessageMap, KernelEvent, MAX_PROJECTION_MESSAGES, ViewContext};
use nmp_core::KernelEventObserver;
use nmp_threading::TimelineBlock;
use serde::{Deserialize, Serialize};

use crate::meta_timeline::{
    ModularTimelinePayload, ModularTimelineSpec, ModularTimelineState, Nip10ModularTimelineView,
};
use crate::note_relations::{NoteRelationCounts, NoteRelationIndex};
use crate::profile_display::{profile_from_event, should_replace, AuthorDisplay, ProfileDisplay};
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
    /// clock. Uses the same `Xs/Xm/Xh/Xd ago` dialect as
    /// [`nmp_nip17::display::format_ago_secs`] and
    /// `nmp_nip29::projection::group_chat::format_ago_secs` — the algorithm
    /// is deliberately micro-duplicated (a NIP crate must not depend on
    /// another NIP crate just to share a trivial helper).
    pub created_at_display: String,
    /// V-27 thin-shell: two-char uppercase initials for the avatar tile,
    /// derived from `author_pubkey`. Mirrors the `pubkey_initials` helper
    /// deleted from `ModularBlockView.swift`.
    pub author_avatar_initials: String,
    /// V-27 thin-shell: deterministic 6-hex avatar background colour
    /// (uppercase, no `#` prefix). djb2 over the last 6 bytes of the pubkey
    /// hex — **byte-identical** to `nmp_nip17::display::avatar_color_hex`,
    /// `nmp_nip29::projection::group_chat::avatar_color_hex`, and
    /// `nmp_marmot::projection::display::avatar_color_hex` so the same
    /// author renders with the same tint across every surface.
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
            author_avatar_initials: pubkey_initials(&event.author),
            author_avatar_color: avatar_color_hex(&event.author),
            author_pubkey_short: pubkey_display(&event.author),
            author_display_name,
            // V-28 thin-shell: same `<first 8>…<last 8>` abbreviation
            // algorithm `author_pubkey_short` uses — `pubkey_display` is
            // generic over any hex string, so we reuse it on `event.id`.
            short_id: pubkey_display(&event.id),
        }
    }
}

// ── V-27 thin-shell display helpers ───────────────────────────────────────
//
// Deliberate micro-duplication of the same algorithms in
// `nmp_nip17::display` and `nmp_nip29::projection::group_chat`. NIP crates
// don't depend on each other just to share trivial display helpers (see
// V-25 / V-22 rationale); the load-bearing property is that the **algorithm
// stays identical** across surfaces so the same author renders the same
// avatar tint and the same abbreviated pubkey everywhere.

fn format_ago_secs(now_secs: u64, then_secs: u64) -> String {
    if then_secs == 0 || now_secs <= then_secs {
        return "now".to_string();
    }
    let diff = now_secs - then_secs;
    if diff < 60 {
        format!("{diff}s ago")
    } else if diff < 3_600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86_400 {
        format!("{}h ago", diff / 3_600)
    } else {
        format!("{}d ago", diff / 86_400)
    }
}

fn now_unix_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn pubkey_initials(hex: &str) -> String {
    hex.chars().take(2).map(|c| c.to_ascii_uppercase()).collect()
}

fn avatar_color_hex(pubkey_hex: &str) -> String {
    let bytes = pubkey_hex.as_bytes();
    let start = bytes.len().saturating_sub(6);
    let tail = &bytes[start..];
    let mut hash: u32 = 5381;
    for b in tail {
        hash = hash.wrapping_mul(33).wrapping_add(u32::from(*b));
    }
    format!("{:06X}", hash & 0x00FF_FFFF)
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

        // initials: first two hex chars uppercased
        assert_eq!(card.author_avatar_initials, "3B");
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

    #[test]
    fn avatar_color_hex_byte_identical_to_nip17() {
        // Pin a known input so the avatar tint stays consistent across surfaces.
        // The same vector is asserted in nmp_nip17 / nmp_nip29 — drifting any
        // of the three would mean the same author renders with a different
        // tint in DMs vs. NIP-29 group chat vs. the modular timeline.
        const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
        // djb2(b"fa459d") = 5381*33^6 + ... (uppercased, masked to 24 bits).
        // Recompute inline against the helper to lock the algorithm.
        let mut hash: u32 = 5381;
        for b in b"fa459d" {
            hash = hash.wrapping_mul(33).wrapping_add(u32::from(*b));
        }
        let expected = format!("{:06X}", hash & 0x00FF_FFFF);
        assert_eq!(avatar_color_hex(PK), expected);
    }

    #[test]
    fn format_ago_secs_buckets_are_stable() {
        assert_eq!(format_ago_secs(0, 0), "now");
        assert_eq!(format_ago_secs(100, 200), "now");
        assert_eq!(format_ago_secs(105, 100), "5s ago");
        assert_eq!(format_ago_secs(160, 100), "1m ago");
        assert_eq!(format_ago_secs(100 + 3_600, 100), "1h ago");
        assert_eq!(format_ago_secs(100 + 86_400, 100), "1d ago");
    }

    #[test]
    fn pubkey_initials_short_inputs_do_not_panic() {
        assert_eq!(pubkey_initials(""), "");
        assert_eq!(pubkey_initials("a"), "A");
        assert_eq!(pubkey_initials("ab"), "AB");
        assert_eq!(pubkey_initials("abcd"), "AB");
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
}
