use egui::{Color32, Ui};

use nmp_gallery_tui::profile_wire::ProfileWire;

/// NIP-05 verified badge.
///
/// Mirrors `NostrNip05Badge` from the TUI registry. Only renders when the
/// profile carries a non-empty NIP-05 identifier.
pub struct Nip05Badge<'a> {
    nip05: &'a str,
}

impl<'a> Nip05Badge<'a> {
    /// Construct from a profile wire. Returns `None` if the profile has no
    /// NIP-05 identifier.
    #[must_use]
    pub fn from_profile(profile: &'a ProfileWire) -> Option<Self> {
        profile.nip05.as_deref().filter(|n| !n.trim().is_empty()).map(|n| Self { nip05: n })
    }

    pub fn show(self, ui: &mut Ui) {
        let bg = Color32::from_rgb(30, 41, 59);
        let fg = Color32::from_rgb(148, 163, 184);
        let text = format!("✓ {}", self.nip05);
        ui.label(
            egui::RichText::new(text)
                .monospace()
                .color(fg)
                .background_color(bg),
        );
    }
}
