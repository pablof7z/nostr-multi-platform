//! Default renderer for kind:0 — profile metadata (NIP-01), the
//! `ProfileProjection` variant of `EmbedKindProjection`. Replace via
//! `NostrKindRegistry::set_profile` for F-CR-11.

use nmp_content::embed_projection::EmbedKindProjection;
use nmp_core::display::short_npub;

use super::super::kind_renderer::KindRenderer;
use super::super::super::nostr_mention_chip::NostrMentionProfileHost;
use super::text_layout::{render_two_line, text_height};
use super::NostrKindRegistry;

/// Default renderer for `ProfileProjection` (kind:0).
/// Shows display name + about. Replace via `registry.set_profile(...)` for F-CR-11.
pub struct DefaultProfileRenderer;

impl KindRenderer for DefaultProfileRenderer {
    fn render(
        &self,
        projection: &EmbedKindProjection,
        _ctx: &nmp_content::context::RenderContext,
        _registry: &NostrKindRegistry,
        _host: Option<&dyn NostrMentionProfileHost>,
        _consumer_id: Option<&str>,
        area: ratatui::layout::Rect,
        buf: &mut ratatui::buffer::Buffer,
    ) {
        let EmbedKindProjection::Profile(profile) = projection else {
            return;
        };
        // The kind:0 is itself the displayed entity here, so its own
        // `display_name` is legitimate profile data — not a separate author
        // claim. Fall back to a Rust-formatted `npub_short`, never raw hex.
        let label = profile
            .display_name
            .clone()
            .unwrap_or_else(|| short_npub(&profile.pubkey));
        let about = profile.about.clone().unwrap_or_default();
        render_two_line("profile", &format!("{label} — {about}"), area, buf);
    }

    fn preferred_height(&self, projection: &EmbedKindProjection, width: u16) -> u16 {
        let EmbedKindProjection::Profile(profile) = projection else {
            return 2;
        };
        let about = profile.about.clone().unwrap_or_default();
        text_height(&about, width).saturating_add(1).max(2)
    }
}
