//! Gallery application state and layout.
//!
//! Static sample profiles — no kernel, no network. Every frame is deterministic.

use iced::widget::{column, row, rule, scrollable, text};
use iced::{Element, Length};

use crate::components::user_avatar::UserAvatar;

/// Static sample profile for gallery rendering.
pub struct SampleProfile {
    pub pubkey: String,
    pub display_name: Option<String>,
    pub about: Option<String>,
}

/// Component gallery app state.
pub struct GalleryApp {
    profiles: Vec<SampleProfile>,
}

impl GalleryApp {
    #[must_use]
    pub fn new() -> Self {
        Self {
            profiles: sample_profiles(),
        }
    }
}

impl Default for GalleryApp {
    fn default() -> Self {
        Self::new()
    }
}

/// Gallery has no interactive messages — it is a static showcase.
#[derive(Debug, Clone)]
pub enum Message {}

pub fn update(_app: &mut GalleryApp, _message: Message) {}

pub fn view(app: &GalleryApp) -> Element<'_, Message> {
    let content = column![
        text("NMP Desktop Component Gallery").size(24),
        rule::horizontal(1),
        text("UserAvatar — deterministic pubkey tint + initials")
            .size(14)
            .style(|_theme: &iced::Theme| text::Style {
                color: Some(iced::Color::from_rgb8(148, 163, 184)),
            }),
        scrollable(avatar_grid(&app.profiles
        ))
        .height(Length::Fill),
    ]
    .spacing(12)
    .padding(16);

    content.into()
}

fn avatar_grid(profiles: &[SampleProfile]) -> Element<'_, Message> {
    let sizes = [24.0, 36.0, 48.0, 64.0];

    let mut grid = column![].spacing(16);

    for size in sizes {
        let size_label = text(format!("{size:.0}px"))
            .font(iced::Font::MONOSPACE)
            .size(12);

        let mut avatars = row![size_label].spacing(8);
        for profile in profiles {
            avatars = avatars.push(
                UserAvatar::new(&profile.pubkey)
                    .display_name(profile.display_name.as_deref())
                    .size(size)
                    .into_element::<Message>(),
            );
        }
        grid = grid.push(avatars);
    }

    grid = grid.push(rule::horizontal(1));
    grid = grid.push(
        text("Fallback — no display name, npub initials")
            .size(14)
            .style(|_theme: &iced::Theme| text::Style {
                color: Some(iced::Color::from_rgb8(148, 163, 184)),
            }),
    );

    let mut fallback_row = row![].spacing(8);
    for profile in profiles.iter().take(3) {
        fallback_row = fallback_row.push(
            UserAvatar::new(&profile.pubkey)
                .display_name(None)
                .size(48.0)
                .into_element::<Message>(),
        );
    }
    grid = grid.push(fallback_row);

    grid.into()
}

fn sample_profiles() -> Vec<SampleProfile> {
    vec![
        SampleProfile {
            pubkey: "a1b2c3d4e5f6789012345678901234567890abcdefabcdefabcdefabcdefabcd".to_string(),
            display_name: Some("Satoshi Nakamoto".to_string()),
            about: Some("Inventor of Bitcoin".to_string()),
        },
        SampleProfile {
            pubkey: "b2c3d4e5f6a7890123456789012345678901abcdefabcdefabcdefabcdefabcde".to_string(),
            display_name: Some("Hal Finney".to_string()),
            about: Some("First Bitcoin recipient".to_string()),
        },
        SampleProfile {
            pubkey: "c3d4e5f6a7b8901234567890123456789012abcdefabcdefabcdefabcdefabcdef".to_string(),
            display_name: Some("Pablo".to_string()),
            about: Some("Building NMP".to_string()),
        },
        SampleProfile {
            pubkey: "d4e5f6a7b8c9012345678901234567890123abcdefabcdefabcdefabcdefabcdefa".to_string(),
            display_name: Some("fiatjaf".to_string()),
            about: Some("Nostr dev".to_string()),
        },
        SampleProfile {
            pubkey: "e5f6a7b8c9d0123456789012345678901234abcdefabcdefabcdefabcdefabcdefab".to_string(),
            display_name: Some("".to_string()),
            about: None,
        },
        SampleProfile {
            pubkey: "f6a7b8c9d0e1234567890123456789012345abcdefabcdefabcdefabcdefabcdefabc".to_string(),
            display_name: None,
            about: None,
        },
    ]
}
