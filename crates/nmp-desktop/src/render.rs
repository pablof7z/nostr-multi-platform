//! Content rendering via `nmp-content`.
//!
//! The shell consumes the `ContentTree` IR directly (Rust→Rust): per
//! ADR-0018 / `docs/design/content-rendering.md` the wire projection exists
//! only for the FFI bridge; in-process we walk `Segment`s natively. Rendering
//! is best-effort (D1) — unresolved mentions/emoji degrade to a readable label
//! rather than failing.

use iced::widget::{button, row, text};
use iced::{Color, Element};
use nmp_content::{tokenize_with_kind, RenderMode, Segment};
use std::borrow::Cow;

use crate::message::Message;

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

/// Parse a `#rrggbb` string into an iced [`Color`], falling back to neutral grey.
pub fn hex_color(hex: &str) -> Color {
    let h = hex.trim_start_matches('#');
    if h.len() == 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&h[0..2], 16),
            u8::from_str_radix(&h[2..4], 16),
            u8::from_str_radix(&h[4..6], 16),
        ) {
            return Color::from_rgb8(r, g, b);
        }
    }
    Color::from_rgb8(120, 120, 120)
}

/// Render a kind:1 note body. Tokenizes through `nmp-content` and lays the
/// segments out as wrapped inline widgets (text, tappable links, hashtags).
pub fn note_body(content: String) -> Element<'static, Message> {
    let tree = tokenize_with_kind(&content, &[], RenderMode::Auto, 1);

    let mut segments = row![].spacing(2);
    for seg in &tree.segments {
        match seg {
            Segment::Text(t) => {
                segments = segments.push(text(t.clone()));
            }
            Segment::Hashtag(tag) => {
                segments = segments.push(
                    text(format!("#{tag}"))
                        .style(|_theme: &iced::Theme| text::Style {
                            color: Some(Color::from_rgb8(96, 165, 250)),
                        }),
                );
            }
            Segment::Url(u) => {
                let url = u.as_str().to_string();
                segments = segments.push(
                    button(text(url.clone()).size(12))
                        .style(|_theme: &iced::Theme, _status| button::Style {
                            ..button::primary(_theme, _status)
                        })
                        .on_press(Message::OpenUrl(url)),
                );
            }
            Segment::Media { urls, .. } => {
                for u in urls {
                    let url = u.as_str().to_string();
                    segments = segments.push(
                        button(text("🖼 media").size(12))
                            .style(|_theme: &iced::Theme, _status| button::Style {
                                ..button::primary(_theme, _status)
                            })
                            .on_press(Message::OpenUrl(url)),
                    );
                }
            }
            Segment::Mention(_) => {
                segments = segments.push(
                    text("@mention")
                        .style(|_theme: &iced::Theme| text::Style {
                            color: Some(Color::from_rgb8(167, 139, 250)),
                        }),
                );
            }
            Segment::EventRef(_) => {
                segments = segments.push(
                    text("↗ note")
                        .style(|_theme: &iced::Theme| text::Style {
                            color: Some(Color::from_rgb8(110, 231, 183)),
                        }),
                );
            }
            Segment::Emoji { shortcode, .. } => {
                segments = segments.push(text(format!(":{shortcode}:")));
            }
            Segment::Invoice(_) => {
                segments = segments.push(
                    text("⚡ invoice")
                        .style(|_theme: &iced::Theme| text::Style {
                            color: Some(Color::from_rgb8(251, 191, 36)),
                        }),
                );
            }
            Segment::MarkdownBlock(_) => {}
        }
    }

    segments.wrap().into()
}
