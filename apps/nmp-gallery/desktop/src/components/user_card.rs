use egui::{Color32, Frame, Ui};

use nmp_gallery_tui::profile_wire::ProfileWire;

use super::user_avatar::UserAvatar;
use super::user_name::UserName;

/// Compact user card — avatar, name, and NIP-05 row.
///
/// Mirrors `NostrUserCard` from the TUI registry.
pub struct UserCard<'a> {
    profile: &'a ProfileWire,
}

impl<'a> UserCard<'a> {
    #[must_use]
    pub fn new(profile: &'a ProfileWire) -> Self {
        Self { profile }
    }

    pub fn show(self, ui: &mut Ui) {
        Frame::group(ui.style())
            .fill(ui.visuals().faint_bg_color)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    UserAvatar::new(self.profile).size(36.0).show(ui);
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        UserName::new(self.profile).show(ui);
                        if let Some(nip05) =
                            self.profile.nip05.as_deref().filter(|n| !n.trim().is_empty())
                        {
                            ui.label(
                                egui::RichText::new(nip05)
                                    .size(11.0)
                                    .color(Color32::from_rgb(148, 163, 184)),
                            );
                        }
                    });
                });
            });
    }
}
