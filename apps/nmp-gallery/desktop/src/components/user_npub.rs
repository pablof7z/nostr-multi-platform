use iced::widget::{container, text};
use iced::{Background, Border, Color, Element, Font, Length, Padding};
use nmp_gallery_tui::profile_wire::ProfileWire;

const BG: Color = Color::from_rgb(30.0 / 255.0, 41.0 / 255.0, 59.0 / 255.0);
const FG: Color = Color::from_rgb(148.0 / 255.0, 163.0 / 255.0, 184.0 / 255.0);

/// Truncated npub in a monospace chip.
pub struct NpubChip {
    npub_short: String,
}

impl NpubChip {
    /// Create an npub chip from a profile.
    #[must_use]
    pub fn from_profile(profile: &ProfileWire) -> Self {
        Self {
            npub_short: profile.npub_short.clone(),
        }
    }

    /// Render the chip as an iced [`Element`].
    pub fn into_element<Message: 'static>(self) -> Element<'static, Message> {
        let npub_short = self.npub_short;
        container(
            text(npub_short)
                .font(Font::MONOSPACE)
                .size(12)
                .style(|_theme| iced::widget::text::Style {
                    color: Some(FG),
                }),
        )
        .padding(Padding::from(6))
        .style(|_theme| container::Style {
            background: Some(Background::Color(BG)),
            border: Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .width(Length::Shrink)
        .into()
    }
}
