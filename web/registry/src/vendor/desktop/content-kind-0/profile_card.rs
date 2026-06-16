use iced::widget::{column, container, row, text};
use iced::{Alignment, Border, Color, Element, Length};

use nmp_content::embed_projection::ProfileProjection;
use nmp_core::display::short_npub;

const MUTED: Color = Color {
    r: 0.580,
    g: 0.639,
    b: 0.722,
    a: 1.0,
};
const FAINT_BG: Color = Color {
    r: 0.071,
    g: 0.098,
    b: 0.141,
    a: 1.0,
};
const BORDER_COLOR: Color = Color {
    r: 0.278,
    g: 0.333,
    b: 0.404,
    a: 1.0,
};

/// Iced profile embed card for kind:0 metadata.
///
/// The caller passes a Rust-owned `ProfileProjection`; this widget only formats
/// raw fields for display and never parses kind:0 JSON.
pub struct ProfileCard<'a> {
    profile: &'a ProfileProjection,
}

impl<'a> ProfileCard<'a> {
    #[must_use]
    pub fn new(profile: &'a ProfileProjection) -> Self {
        Self { profile }
    }

    pub fn into_element<Message: 'static>(self) -> Element<'a, Message> {
        let profile = self.profile;
        let label = profile
            .display_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| short_npub(&profile.pubkey));

        let mut body = column![
            row![
                text(label)
                    .size(15)
                    .font(iced::Font {
                        weight: iced::font::Weight::Bold,
                        ..iced::Font::default()
                    })
                    .style(|_| iced::widget::text::Style {
                        color: Some(Color::from_rgb8(241, 245, 249)),
                    }),
                text("kind:0")
                    .size(10)
                    .style(|_| iced::widget::text::Style { color: Some(MUTED) }),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            text(short_npub(&profile.pubkey))
                .size(11)
                .style(|_| iced::widget::text::Style { color: Some(MUTED) }),
        ]
        .spacing(6);

        if let Some(nip05) = profile
            .nip05
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            body = body.push(
                text(nip05.strip_prefix("_@").unwrap_or(nip05))
                    .size(12)
                    .style(|_| iced::widget::text::Style {
                        color: Some(Color::from_rgb8(110, 231, 183)),
                    }),
            );
        }

        if let Some(about) = profile
            .about
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            body = body.push(
                text(about)
                    .size(12)
                    .style(|_| iced::widget::text::Style { color: Some(MUTED) }),
            );
        }

        container(body)
            .width(Length::Fill)
            .padding(12)
            .style(|_| container::Style {
                background: Some(iced::Background::Color(FAINT_BG)),
                border: Border {
                    color: BORDER_COLOR,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            })
            .into()
    }
}
