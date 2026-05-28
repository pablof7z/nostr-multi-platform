//! Gallery application state and layout.
//!
//! Static sample profiles — no kernel, no network. Every frame is deterministic.

use eframe::App;
use egui::{CentralPanel, Color32, ScrollArea, Ui};

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

impl App for GalleryApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        CentralPanel::default().show(ctx, |ui| {
            ui.heading("NMP Desktop Component Gallery");
            ui.separator();

            ui.label(
                egui::RichText::new("UserAvatar — deterministic pubkey tint + initials")
                    .color(Color32::from_rgb(148, 163, 184))
                    .size(14.0),
            );
            ui.add_space(12.0);

            ScrollArea::vertical().show(ui, |ui| {
                avatar_grid(ui, &self.profiles);
            });
        });
    }
}

fn avatar_grid(ui: &mut Ui, profiles: &[SampleProfile]) {
    let sizes = [24.0, 36.0, 48.0, 64.0];

    for size in sizes {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{size:.0}px")).strong().monospace());
            ui.add_space(8.0);

            for profile in profiles {
                ui.add_space(4.0);
                UserAvatar::new(&profile.pubkey)
                    .display_name(profile.display_name.as_deref())
                    .size(size)
                    .show(ui);
            }
        });
        ui.add_space(16.0);
    }

    ui.separator();
    ui.label(
        egui::RichText::new("Fallback — no display name, npub initials")
            .color(Color32::from_rgb(148, 163, 184))
            .size(14.0),
    );
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        for profile in profiles.iter().take(3) {
            UserAvatar::new(&profile.pubkey)
                .display_name(None)
                .size(48.0)
                .show(ui);
            ui.add_space(8.0);
        }
    });
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
