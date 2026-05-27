use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use super::{content_render_data::ContentProfileRenderData, content_tree_wire::WireUri};

/// Inline terminal chip for a pre-resolved profile mention.
pub struct NostrMentionChip<'a> {
    uri: &'a WireUri,
    profile: Option<&'a ContentProfileRenderData>,
    style: Style,
}

impl<'a> NostrMentionChip<'a> {
    pub fn new(uri: &'a WireUri) -> Self {
        Self {
            uri,
            profile: None,
            style: Style::default()
                .fg(mention_color(&uri.primary_id))
                .add_modifier(Modifier::BOLD),
        }
    }

    pub fn profile(mut self, profile: Option<&'a ContentProfileRenderData>) -> Self {
        self.profile = profile;
        self
    }

    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub fn label(&self) -> String {
        let raw = self
            .profile
            .map(ContentProfileRenderData::label)
            .unwrap_or(&self.uri.primary_id);
        let label = self
            .profile
            .and_then(|profile| profile.display_name.as_deref())
            .map(str::to_string)
            .unwrap_or_else(|| short_id(raw));
        format!("@{label}")
    }

    pub fn span(&self) -> Span<'static> {
        Span::styled(self.label(), self.style)
    }
}

impl Widget for NostrMentionChip<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(Line::from(self.span())).render(area, buf);
    }
}

fn short_id(id: &str) -> String {
    let count = id.chars().count();
    if count <= 12 {
        id.to_string()
    } else {
        let head = id.chars().take(6).collect::<String>();
        let tail = id
            .chars()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>();
        format!("{head}…{tail}")
    }
}

fn mention_color(id: &str) -> Color {
    let hue = stable_hash(id) % 360;
    let (r, g, b) = hsl_to_rgb(hue as f32, 0.72, 0.66);
    Color::Rgb(r, g, b)
}

fn stable_hash(value: &str) -> u32 {
    value.bytes().fold(5381u32, |hash, byte| {
        hash.wrapping_mul(33) ^ u32::from(byte)
    })
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h_prime = h / 60.0;
    let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match h_prime {
        h if (0.0..1.0).contains(&h) => (c, x, 0.0),
        h if (1.0..2.0).contains(&h) => (x, c, 0.0),
        h if (2.0..3.0).contains(&h) => (0.0, c, x),
        h if (3.0..4.0).contains(&h) => (0.0, x, c),
        h if (4.0..5.0).contains(&h) => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    (channel(r1 + m), channel(g1 + m), channel(b1 + m))
}

fn channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mention_color_is_deterministic_per_pubkey() {
        let first =
            mention_color("1111111111111111111111111111111111111111111111111111111111111111");
        let second =
            mention_color("1111111111111111111111111111111111111111111111111111111111111111");
        let other =
            mention_color("2222222222222222222222222222222222222222222222222222222222222222");
        assert_eq!(first, second);
        assert_ne!(first, other);
    }
}
