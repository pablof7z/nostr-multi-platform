use egui::Ui;

use nmp_gallery_tui::profile_wire::ProfileWire;

/// Profile name label — display name with npub fallback.
///
/// Mirrors `NostrProfileName` from the TUI registry. If `display_name` is
/// present and non-empty it is rendered strong; otherwise the short npub
/// is rendered weak.
pub struct UserName<'a> {
    profile: &'a ProfileWire,
}

impl<'a> UserName<'a> {
    #[must_use]
    pub fn new(profile: &'a ProfileWire) -> Self {
        Self { profile }
    }

    pub fn show(self, ui: &mut Ui) {
        let label = self.profile.display_name.as_deref().filter(|n| !n.trim().is_empty());
        if let Some(name) = label {
            ui.label(egui::RichText::new(name).strong());
        } else {
            ui.label(egui::RichText::new(&self.profile.npub_short).weak());
        }
    }
}
