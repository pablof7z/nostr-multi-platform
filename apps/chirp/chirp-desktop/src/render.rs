//! Content rendering via `nmp-content`.
//!
//! The shell consumes the `ContentTree` IR directly (Rust→Rust): per
//! ADR-0018 the wire projection exists only for the FFI bridge; in-process
//! we walk `Segment`s natively. Rendering is best-effort (D1).

use egui::{Color32, RichText, Ui};
use nmp_content::{tokenize_with_kind, RenderMode, Segment};
use std::borrow::Cow;

/// Extract a human-readable content string from a raw timeline content field.
///
/// Kind:6 reposts carry the full JSON of the reposted event as `content`.
/// Best-effort (D1): if `content` parses as a JSON object with a `"content"`
/// string, use that inner text; otherwise fall back to the raw string.
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
pub fn note_body(ui: &mut Ui, content: &str) {
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
                Segment::MarkdownBlock(_) => {}
            }
        }
    });
}
