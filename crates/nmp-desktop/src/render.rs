//! Content rendering via `nmp-content`.
//!
//! The shell consumes the `ContentTree` IR directly (Rust→Rust): per
//! ADR-0018 / `docs/design/content-rendering.md` the wire projection exists
//! only for the FFI bridge; in-process we walk `Segment`s natively. Rendering
//! is best-effort (D1) — unresolved mentions/emoji degrade to a readable label
//! rather than failing.

use egui::{Color32, RichText, Ui};
use nmp_content::{tokenize_with_kind, RenderMode, Segment};
use std::borrow::Cow;

/// Extract a human-readable content string from a raw timeline content field.
///
/// Kind:6 reposts carry the full JSON of the reposted event as `content`
/// (a kernel-wide behaviour; reported across all NMP platforms). Best-effort
/// (D1): if `content` parses as a JSON object with a `"content"` string, use
/// that inner text; otherwise fall back to the raw string.
/// Returns `(text, is_repost)`.
pub fn effective_content(raw: &str) -> (Cow<'_, str>, bool) {
    if raw.starts_with('{') {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
            if let Some(inner) = v["content"].as_str() {
                return (Cow::Owned(inner.to_string()), true);
            }
        }
        return (Cow::Borrowed(""), true);
    }
    (Cow::Borrowed(raw), false)
}

/// Parse a `#rrggbb` string (the kernel's deterministic per-author colour)
/// into an egui colour, falling back to a neutral grey.
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

/// Render a kind:1 note body. Tokenizes through `nmp-content` and lays the
/// segments out as wrapped inline widgets (text, tappable links, hashtags).
pub fn note_body(ui: &mut Ui, content: &str) {
    // kind:1 → Plain mode (sniffed); empty tags: in-process timeline rows do
    // not carry the event's tag vector, so emoji/mention resolution is
    // best-effort by design (D1).
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
                Segment::Mention(_) => {
                    ui.label(
                        RichText::new("@mention").color(Color32::from_rgb(167, 139, 250)),
                    );
                }
                Segment::EventRef(_) => {
                    ui.label(
                        RichText::new("↗ note").color(Color32::from_rgb(110, 231, 183)),
                    );
                }
                Segment::Emoji { shortcode, .. } => {
                    ui.label(format!(":{shortcode}:"));
                }
                Segment::Invoice(_) => {
                    ui.label(RichText::new("⚡ invoice").color(Color32::from_rgb(251, 191, 36)));
                }
                // kind:1 resolves to Plain, so block markdown never appears
                // here; if a future kind routes through, skip rather than
                // panic (D1/D6).
                Segment::MarkdownBlock(_) => {}
            }
        }
    });
}
