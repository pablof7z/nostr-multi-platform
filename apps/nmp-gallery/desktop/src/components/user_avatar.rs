use iced::widget::container;
use iced::widget::image::Handle;
use iced::ContentFit;
use iced::{Border, Color, Element, Length};

/// Circular avatar widget with deterministic pubkey-derived tint.
///
/// When `picture_bytes` are supplied the image is rendered as a circle (via
/// container clip + border-radius). Otherwise renders a tinted circle with
/// initials, identical to the TUI surface.
pub struct UserAvatar {
    pubkey_hex: String,
    display_name: Option<String>,
    picture_bytes: Option<Vec<u8>>,
    size: f32,
}

impl UserAvatar {
    #[must_use]
    pub fn new(pubkey_hex: &str) -> Self {
        Self {
            pubkey_hex: pubkey_hex.to_string(),
            display_name: None,
            picture_bytes: None,
            size: 36.0,
        }
    }

    #[must_use]
    pub fn display_name(mut self, name: Option<&str>) -> Self {
        self.display_name = name.map(String::from);
        self
    }

    /// Supply decoded image bytes. When present, renders the actual profile
    /// picture (clipped to a circle) instead of the initials fallback.
    #[must_use]
    pub fn picture_bytes(mut self, bytes: &[u8]) -> Self {
        self.picture_bytes = Some(bytes.to_vec());
        self
    }

    #[must_use]
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn into_element<Message: 'static>(self) -> Element<'static, Message> {
        let size = self.size;

        if let Some(bytes) = self.picture_bytes {
            // Render actual profile picture clipped to a circle.
            let handle = Handle::from_bytes(bytes);
            container(
                iced::widget::image(handle)
                    .width(Length::Fixed(size))
                    .height(Length::Fixed(size))
                    .content_fit(ContentFit::Cover),
            )
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .clip(true)
            .style(move |_| container::Style {
                border: Border {
                    radius: (size / 2.0).into(),
                    color: Color::TRANSPARENT,
                    width: 0.0,
                },
                ..Default::default()
            })
            .into()
        } else {
            // Deterministic tinted circle with initials.
            let color = hex_color(&nmp_core::display::avatar_color_hex(&self.pubkey_hex));
            let initials = if let Some(ref name) = self.display_name {
                nmp_core::display::display_name_initials(name)
            } else {
                let npub = nmp_core::display::to_npub(&self.pubkey_hex);
                nmp_core::display::avatar_initials(&npub)
            };

            container(
                iced::widget::text(initials)
                    .size(size * 0.4)
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center),
            )
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(color)),
                border: Border {
                    radius: (size / 2.0).into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
        }
    }
}

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
