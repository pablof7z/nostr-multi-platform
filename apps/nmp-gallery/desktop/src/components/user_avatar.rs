use egui::{Color32, Response, Ui, Vec2};

use nmp_gallery_tui::profile_wire::ProfileWire;

/// Circular avatar widget with deterministic pubkey-derived tint.
///
/// Renders a colored circle containing the author's initials. Uses the
/// canonical `nmp_core::display` helpers for colour and initials so the
/// desktop surface agrees byte-for-byte with TUI / iOS / Android.
///
/// # Example
/// ```ignore
/// UserAvatar::new(&profile).size(48.0).show(ui);
/// ```
pub struct UserAvatar<'a> {
    profile: &'a ProfileWire,
    size: f32,
}

impl<'a> UserAvatar<'a> {
    /// Create a new avatar for the given profile wire.
    #[must_use]
    pub fn new(profile: &'a ProfileWire) -> Self {
        Self { profile, size: 36.0 }
    }

    /// Set the diameter of the avatar circle in points. Default is `36.0`.
    #[must_use]
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    /// Render the avatar into the given [`Ui`] and return the [`Response`].
    pub fn show(self, ui: &mut Ui) -> Response {
        let (rect, response) =
            ui.allocate_exact_size(Vec2::splat(self.size), egui::Sense::hover());
        if !ui.is_rect_visible(rect) {
            return response;
        }

        let color = hex_color(&nmp_core::display::avatar_color_hex(&self.profile.pubkey));
        let initials = if let Some(ref name) = self.profile.display_name {
            if !name.trim().is_empty() {
                nmp_core::display::display_name_initials(name)
            } else {
                nmp_core::display::avatar_initials(&self.profile.npub)
            }
        } else {
            nmp_core::display::avatar_initials(&self.profile.npub)
        };

        let painter = ui.painter();
        let radius = self.size / 2.0;
        painter.circle_filled(rect.center(), radius, color);

        let font_size = self.size * 0.4;
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            &initials,
            egui::FontId::proportional(font_size),
            Color32::WHITE,
        );

        response
    }
}

fn hex_color(hex: &str) -> Color32 {
    let h = hex.trim_start_matches('#');
    if h.len() == 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&h[0..2], 16),
            u8::from_str_radix(&h[2..4], 16),
            u8::from_str_radix(&h[4..6], 16),
        ) {
            return Color32::from_rgb(r, g, b);
        }
    }
    Color32::from_gray(120)
}
