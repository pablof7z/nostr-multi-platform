use iced::widget::{container, text};
use iced::{Element, Length};

/// Circular avatar widget with deterministic pubkey-derived tint.
///
/// Renders a colored circle containing the author's initials. If a display
/// name is provided, initials are computed from the name; otherwise they
/// fall back to the first two characters of the bech32 npub body.
///
/// # Example
/// ```ignore
/// UserAvatar::new(&profile.pubkey)
///     .display_name(profile.display_name.as_deref())
///     .size(48.0)
///     .into_element();
/// ```
pub struct UserAvatar {
    pubkey_hex: String,
    display_name: Option<String>,
    size: f32,
}

impl UserAvatar {
    /// Create a new avatar for the given hex pubkey.
    #[must_use]
    pub fn new(pubkey_hex: &str) -> Self {
        Self {
            pubkey_hex: pubkey_hex.to_string(),
            display_name: None,
            size: 36.0,
        }
    }

    /// Set the display name used for initial generation.
    #[must_use]
    pub fn display_name(mut self, name: Option<&str>) -> Self {
        self.display_name = name.map(String::from);
        self
    }

    /// Set the diameter of the avatar circle in points. Default is `36.0`.
    #[must_use]
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    /// Render the avatar as an iced [`Element`].
    pub fn into_element<Message: 'static>(self) -> Element<'static, Message> {
        let color = hex_color(&nmp_core::display::avatar_color_hex(&self.pubkey_hex));
        let initials = if let Some(ref name) = self.display_name {
            nmp_core::display::display_name_initials(name)
        } else {
            let npub = nmp_core::display::to_npub(&self.pubkey_hex);
            nmp_core::display::avatar_initials(&npub)
        };

        let size = self.size;
        container(
            text(initials)
                .size(size * 0.4)
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center),
        )
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_theme: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(color)),
            border: iced::Border {
                radius: (size / 2.0).into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
    }
}

/// Parse a `#rrggbb` string into an iced [`Color`].
fn hex_color(hex: &str) -> iced::Color {
    let h = hex.trim_start_matches('#');
    if h.len() == 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&h[0..2], 16),
            u8::from_str_radix(&h[2..4], 16),
            u8::from_str_radix(&h[4..6], 16),
        ) {
            return iced::Color::from_rgb8(r, g, b);
        }
    }
    iced::Color::from_rgb8(120, 120, 120)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_renders_with_display_name_initials() {
        let avatar = UserAvatar::new("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789")
            .display_name(Some("Alice Smith"))
            .size(48.0);
        assert_eq!(avatar.display_name, Some("Alice Smith".to_string()));
        assert_eq!(avatar.size, 48.0);
        // Smoke: element generation must not panic.
        let _ = avatar.into_element::<()>();
    }

    #[test]
    fn avatar_falls_back_to_npub_initials_when_no_name() {
        let avatar = UserAvatar::new("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789");
        assert!(avatar.display_name.is_none());
        assert_eq!(avatar.size, 36.0);
        let _ = avatar.into_element::<()>();
    }
}
