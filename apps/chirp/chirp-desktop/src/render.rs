//! Content rendering via `nmp-content`.
//!
//! The shell consumes the `ContentTree` IR directly (Rust→Rust): per
//! ADR-0018 the wire projection exists only for the FFI bridge; in-process
//! we walk `Segment`s natively. Rendering is best-effort (D1).

use std::collections::HashMap;

use egui::{Color32, RichText, Ui};
use nmp_content::{
    tokenize_with_kind, EmbedKindProjection, EmbeddedEventEnvelope, RenderMode, Segment,
};
use nmp_core::nip21::NostrUri;

use crate::snapshot::ProfileCard;

/// Parse a `#rrggbb` string into an egui colour, falling back to neutral grey.
pub fn hex_color(hex: &str) -> Color32 {
    let h = hex.trim_start_matches('#');
    if h.len() == 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&h[0..2], 16),
            u8::from_str_radix(&h[2..4], 16),
            u8::from_str_radix(&h[4..6], 16),
        ) {
            return Color32::from_rgb(r, g, b);
        }
    }
    Color32::from_gray(120)
}

/// Render a kind:1 note body as wrapped inline widgets.
///
/// `embeds` is the pre-resolved `primary_id -> EmbeddedEventEnvelope` map from
/// the typed `refs.event.envelopes` sidecar (issue #1283 Phase 1). An `EventRef`
/// segment whose `primary_id` is present in the map renders the embedded event
/// (kernel-resolved, never re-parsed here — D0 thin-shell); an absent one falls
/// back to the `↗ note` placeholder (the kernel has not claimed it yet).
pub fn note_body(
    ui: &mut Ui,
    content: &str,
    embeds: &HashMap<String, EmbeddedEventEnvelope>,
    profiles: &HashMap<String, ProfileCard>,
) {
    let tree = tokenize_with_kind(content, &[], RenderMode::Auto, 1);

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for seg in &tree.segments {
            match seg {
                Segment::Text(t) => {
                    ui.label(t);
                }
                Segment::Hashtag(tag) => {
                    ui.label(
                        RichText::new(format!("#{tag}")).color(Color32::from_rgb(96, 165, 250)),
                    );
                }
                Segment::Url(u) => {
                    ui.hyperlink(u.as_str());
                }
                Segment::Media { urls, .. } => {
                    for u in urls {
                        ui.hyperlink_to("🖼 media", u.as_str());
                    }
                }
                Segment::Mention(uri) => {
                    ui.label(
                        RichText::new(mention_label(uri, profiles))
                            .color(Color32::from_rgb(167, 139, 250)),
                    );
                }
                Segment::EventRef(uri) => {
                    render_event_ref(ui, uri, embeds);
                }
                Segment::Emoji { shortcode, .. } => {
                    ui.label(format!(":{shortcode}:"));
                }
                Segment::Invoice(_) => {
                    ui.label(RichText::new("⚡ invoice").color(Color32::from_rgb(251, 191, 36)));
                }
                Segment::MarkdownBlock(_) => {}
            }
        }
    });
}

/// ADR-0063 (#1671 Lane F) — collect every profile-mention pubkey the note body
/// renders, using the SAME tokenisation `note_body` walks so the resolved set
/// matches exactly what's on screen (a mention the renderer draws via
/// `mention_label` is exactly a `Segment::Mention(NostrUri::Profile{pubkey})`).
/// Raw hex only (ADR-0032). Best-effort (D1): unparseable content yields nothing.
pub fn collect_body_mention_pubkeys(content: &str, out: &mut std::collections::HashSet<String>) {
    if content.is_empty() {
        return;
    }
    let tree = tokenize_with_kind(content, &[], RenderMode::Auto, 1);
    for seg in &tree.segments {
        if let Segment::Mention(NostrUri::Profile { pubkey, .. }) = seg {
            if !pubkey.is_empty() {
                out.insert(pubkey.clone());
            }
        }
    }
}

fn mention_label(uri: &NostrUri, profiles: &HashMap<String, ProfileCard>) -> String {
    let NostrUri::Profile { pubkey, .. } = uri else {
        return "@mention".to_string();
    };
    let label = profiles
        .get(pubkey)
        .and_then(|profile| profile.display_name.as_deref())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| nmp_core::display::short_npub(pubkey));
    format!("@{label}")
}

/// The `primary_id` the embed sidecar keys an `EventRef` by: the event id for a
/// note/`nevent`, or the `kind:pubkey:identifier` coordinate for an addressable
/// (`naddr`) ref. Mirrors the kernel's claim-coordinate keying. `Profile` refs
/// have no embed entry (they are mentions, not event embeds) → `None`.
fn event_ref_primary_id(uri: &NostrUri) -> Option<String> {
    match uri {
        NostrUri::Event { event_id, .. } => Some(event_id.clone()),
        NostrUri::Address {
            identifier,
            pubkey,
            kind,
            ..
        } => Some(format!("{kind}:{pubkey}:{identifier}")),
        NostrUri::Profile { .. } => None,
    }
}

/// Render a single resolved embed inline. Reads ONLY the kernel-resolved
/// projection (no tag/JSON parsing here — that is the kernel's job, #1283).
fn render_event_ref(ui: &mut Ui, uri: &NostrUri, embeds: &HashMap<String, EmbeddedEventEnvelope>) {
    let Some(envelope) = event_ref_primary_id(uri).and_then(|id| embeds.get(&id)) else {
        // Not claimed/resolved yet — keep the placeholder.
        ui.label(RichText::new("↗ note").color(Color32::from_rgb(110, 231, 183)));
        return;
    };
    let accent = Color32::from_rgb(110, 231, 183);
    match &envelope.projection {
        EmbedKindProjection::ShortNote(n) => {
            ui.label(RichText::new(format!(
                "↳ {}",
                truncate(&n.content_tree_text(), 160)
            )));
        }
        EmbedKindProjection::Article(a) => {
            let title = a.title.as_deref().unwrap_or("article");
            ui.label(RichText::new(format!("📄 {title}")).color(accent));
        }
        EmbedKindProjection::Highlight(h) => {
            ui.label(RichText::new(format!("“{}”", truncate(&h.highlighted_text, 160))).italics());
        }
        EmbedKindProjection::Profile(p) => {
            let name = p.display_name.as_deref().unwrap_or(&p.pubkey);
            ui.label(RichText::new(format!("👤 {name}")).color(accent));
        }
        EmbedKindProjection::Unknown(u) => {
            ui.label(
                RichText::new(format!("kind:{} {}", u.kind, truncate(&u.content, 120)))
                    .color(accent),
            );
        }
    }
}

/// Truncate to `limit` chars with an ellipsis (display-only helper).
fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_string()
    } else {
        let mut out: String = text.chars().take(limit).collect();
        out.push('…');
        out
    }
}

/// Plain-text flattening of a `ShortNoteProjection`'s content tree for the
/// inline embed preview. The Rust resolver already produced the tree; this just
/// concatenates its `Text` segment leaves (no re-tokenisation).
trait ContentTreeText {
    fn content_tree_text(&self) -> String;
}

impl ContentTreeText for nmp_content::ShortNoteProjection {
    fn content_tree_text(&self) -> String {
        use nmp_content::WireNode;
        self.content_tree
            .nodes
            .iter()
            .filter_map(|node| match node {
                WireNode::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_ref_primary_id_uses_event_id_for_note() {
        let uri = NostrUri::Event {
            event_id: "ab".repeat(32),
            relays: vec![],
            author: None,
            kind: None,
        };
        assert_eq!(event_ref_primary_id(&uri), Some("ab".repeat(32)));
    }

    #[test]
    fn event_ref_primary_id_uses_coordinate_for_address() {
        let uri = NostrUri::Address {
            identifier: "my-article".to_string(),
            pubkey: "cd".repeat(32),
            kind: 30023,
            relays: vec![],
        };
        assert_eq!(
            event_ref_primary_id(&uri),
            Some(format!("30023:{}:my-article", "cd".repeat(32))),
            "addressable refs key by kind:pubkey:identifier (the kernel claim coordinate)"
        );
    }

    #[test]
    fn event_ref_primary_id_is_none_for_profile() {
        let uri = NostrUri::Profile {
            pubkey: "ef".repeat(32),
            relays: vec![],
        };
        assert_eq!(
            event_ref_primary_id(&uri),
            None,
            "profile refs are not event embeds"
        );
    }

    #[test]
    fn mention_label_uses_resolved_profile_name() {
        let pubkey = "ef".repeat(32);
        let uri = NostrUri::Profile {
            pubkey: pubkey.clone(),
            relays: vec![],
        };
        let mut profiles = HashMap::new();
        profiles.insert(
            pubkey,
            ProfileCard {
                display_name: Some("Alice".to_string()),
                ..Default::default()
            },
        );

        assert_eq!(mention_label(&uri, &profiles), "@Alice");
    }

    #[test]
    fn mention_label_falls_back_to_short_npub() {
        let pubkey = "ef".repeat(32);
        let uri = NostrUri::Profile {
            pubkey: pubkey.clone(),
            relays: vec![],
        };
        let profiles = HashMap::new();

        assert_eq!(
            mention_label(&uri, &profiles),
            format!("@{}", nmp_core::display::short_npub(&pubkey))
        );
    }

    #[test]
    fn collect_body_mention_pubkeys_extracts_rendered_profile_mention() {
        // The SAME tokenisation note_body walks: a `nostr:npub…` in the body is
        // a Segment::Mention(Profile{pubkey}), so the resolved set must contain
        // exactly that pubkey (matching what mention_label draws on screen).
        let pubkey = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
        let npub = nmp_core::nip19::encode_npub(pubkey).expect("fixture npub encodes");
        let content = format!("hello nostr:{npub} how are you");

        let mut out = std::collections::HashSet::new();
        collect_body_mention_pubkeys(&content, &mut out);

        assert!(
            out.contains(pubkey),
            "a rendered profile mention must be collected so its ref resolves"
        );
        assert_eq!(out.len(), 1, "only the mentioned pubkey, nothing else");
    }

    #[test]
    fn collect_body_mention_pubkeys_ignores_plain_text() {
        let mut out = std::collections::HashSet::new();
        collect_body_mention_pubkeys("just a plain note with no mentions", &mut out);
        assert!(out.is_empty(), "no mention segments → nothing to resolve");
    }

    #[test]
    fn truncate_keeps_short_text_and_ellipsizes_long() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hello…");
    }
}
