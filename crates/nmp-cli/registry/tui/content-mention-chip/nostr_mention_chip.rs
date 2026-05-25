use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use super::{
    content_render_data::ContentProfileRenderData, content_tree_wire::WireUri,
};

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
                .fg(Color::Rgb(125, 211, 252))
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
