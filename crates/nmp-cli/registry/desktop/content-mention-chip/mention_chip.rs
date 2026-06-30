use iced::widget::{container, text};
use iced::{Border, Element};

use nmp_gallery_tui::content_render_data::ContentProfileRenderData;
use nmp_gallery_tui::content_tree_wire::WireUri;

use super::content_core::{short_id, ACCENT, BORDER_COLOR, SURFACE_2};

pub struct NostrMentionChip {
    label: String,
}

impl NostrMentionChip {
    #[must_use]
    pub fn new(uri: &WireUri) -> Self {
        Self {
            label: format!("@{}", short_id(&uri.primary_id)),
        }
    }

    #[must_use]
    pub fn profile(mut self, profile: Option<&ContentProfileRenderData>) -> Self {
        if let Some(profile) = profile {
            self.label = format!("@{}", profile.label());
        }
        self
    }

    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        let label = label.into();
        self.label = if label.starts_with('@') {
            label
        } else {
            format!("@{label}")
        };
        self
    }

    pub fn into_element<Message: 'static>(self) -> Element<'static, Message> {
        container(text(self.label).size(13).style(|_| text::Style {
            color: Some(ACCENT),
        }))
        .padding([6, 10])
        .style(|_| container::Style {
            background: Some(iced::Background::Color(SURFACE_2)),
            border: Border {
                color: BORDER_COLOR,
                width: 1.0,
                radius: 999.0.into(),
            },
            ..Default::default()
        })
        .into()
    }
}
