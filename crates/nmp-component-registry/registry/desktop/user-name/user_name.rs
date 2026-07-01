use iced::widget::text;
use iced::{Color, Element};
use super::profile_wire::ProfileWire;

/// Display name with npub fallback.
///
/// Clones display data at construction so the element lifetime is `'static`.
pub struct UserName {
    display_name: Option<String>,
    npub_short: String,
}

impl UserName {
    #[must_use]
    pub fn from_profile(profile: &ProfileWire) -> Self {
        Self {
            display_name: profile.display_name.clone(),
            npub_short: profile.npub_short.clone(),
        }
    }

    pub fn into_element<Message: 'static>(self) -> Element<'static, Message> {
        let has_name = self
            .display_name
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);

        if has_name {
            let name = self.display_name.unwrap_or_default();
            text(name)
                .size(16)
                .font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..iced::Font::default()
                })
                .into()
        } else {
            const MUTED: Color = Color::from_rgb(0.6, 0.6, 0.6);
            text(self.npub_short)
                .size(16)
                .style(move |_theme| iced::widget::text::Style {
                    color: Some(MUTED),
                })
                .into()
        }
    }
}
