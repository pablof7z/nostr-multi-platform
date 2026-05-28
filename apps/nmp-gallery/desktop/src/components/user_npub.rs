use egui::{Color32, Ui};

use nmp_gallery_tui::profile_wire::ProfileWire;

/// Truncated npub identity chip.
///
/// Mirrors `NostrNpubChip` from the TUI registry.
pub struct NpubChip<'a> {
    profile: &'a ProfileWire,
}

impl<'a> NpubChip<'a> {
    #[must_use]
    pub fn new(profile: &'a ProfileWire) -> Self {
        Self { profile }
    }

    pub fn show(self, ui: &mut Ui) {
        let bg = Color32::from_rgb(30, 41, 59);
        let fg = Color32::from_rgb(148, 163, 184);
        ui.label(
            egui::RichText::new(&self.profile.npub_short)
                .monospace()
                .color(fg)
                .background_color(bg),
        );
    }
}
