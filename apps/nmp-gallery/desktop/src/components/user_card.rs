use iced::widget::image::Handle as ImageHandle;
use iced::widget::{column, row, text};
use iced::{Alignment, Color, Element};
use nmp_gallery_tui::profile_wire::ProfileWire;

use super::user_avatar::UserAvatar;

const GREEN: Color = Color::from_rgb(110.0 / 255.0, 231.0 / 255.0, 183.0 / 255.0);

/// Avatar + name + optional NIP-05 row.
///
/// Clones display data at construction so the element lifetime is `'static`.
pub struct UserCard {
    pubkey: String,
    display_name: Option<String>,
    npub_short: String,
    nip05: Option<String>,
    avatar_handle: Option<ImageHandle>,
}

impl UserCard {
    #[must_use]
    pub fn from_profile(profile: &ProfileWire) -> Self {
        Self {
            pubkey: profile.pubkey.clone(),
            display_name: profile.display_name.clone(),
            npub_short: profile.npub_short.clone(),
            nip05: profile.nip05.clone(),
            avatar_handle: None,
        }
    }

    /// Forward the pre-built image handle so the embedded `UserAvatar`
    /// renders the real profile picture instead of the initials fallback.
    #[must_use]
    pub fn avatar_handle(mut self, handle: ImageHandle) -> Self {
        self.avatar_handle = Some(handle);
        self
    }

    pub fn into_element<Message: 'static>(self) -> Element<'static, Message> {
        let mut av = UserAvatar::new(&self.pubkey)
            .display_name(self.display_name.as_deref())
            .size(40.0);
        if let Some(handle) = self.avatar_handle {
            av = av.picture_handle(handle);
        }
        let avatar = av.into_element();

        let has_name = self
            .display_name
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);

        let name_text: Element<'static, Message> = if has_name {
            text(self.display_name.unwrap_or_default())
                .size(14)
                .font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..iced::Font::default()
                })
                .into()
        } else {
            text(self.npub_short).size(14).into()
        };

        let mut label_col = column![name_text].spacing(2);

        if let Some(nip05) = self
            .nip05
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
        {
            let nip05_row = row![
                text("✓")
                    .size(12)
                    .style(|_theme| iced::widget::text::Style { color: Some(GREEN) }),
                text(nip05)
                    .size(12)
                    .style(|_theme| iced::widget::text::Style { color: Some(GREEN) }),
            ]
            .spacing(2);
            label_col = label_col.push(nip05_row);
        }

        row![avatar, label_col]
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
    }
}
