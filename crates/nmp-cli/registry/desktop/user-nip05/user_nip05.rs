use iced::widget::{row, text};
use iced::{Color, Element};
use super::profile_wire::ProfileWire;

const GREEN: Color = Color::from_rgb(
    110.0 / 255.0,
    231.0 / 255.0,
    183.0 / 255.0,
);

/// NIP-05 verified domain badge.
///
/// Returns `None` when the profile has no NIP-05 identifier.
pub struct Nip05Badge {
    nip05: String,
}

impl Nip05Badge {
    /// Create a badge from a profile, or `None` if no NIP-05 is present.
    #[must_use]
    pub fn from_profile(profile: &ProfileWire) -> Option<Self> {
        let nip05 = profile
            .nip05
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())?;
        Some(Self {
            nip05: nip05.to_string(),
        })
    }

    /// Render the badge as an iced [`Element`].
    ///
    /// A leading `_@` (the NIP-05 "root" identifier convention) is elided so
    /// `_@f7z.io` renders as the bare domain `f7z.io`; a normal
    /// `name@domain` identifier renders verbatim.
    pub fn into_element<Message: 'static>(self) -> Element<'static, Message> {
        let nip05 = self
            .nip05
            .strip_prefix("_@")
            .map_or(self.nip05.clone(), str::to_string);
        row![
            text("✓").size(13).style(|_theme| iced::widget::text::Style {
                color: Some(GREEN),
            }),
            text(nip05).size(13).style(|_theme| iced::widget::text::Style {
                color: Some(GREEN),
            }),
        ]
        .spacing(2)
        .into()
    }
}
