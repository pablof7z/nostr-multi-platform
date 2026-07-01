use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use super::{content_render_data::ContentProfileRenderData, content_tree_wire::WireUri};

/// Host bridge for profile projections owned by the NMP kernel.
///
/// Immediate-mode mention widgets call this while rendering visible profile
/// references. The host supplies the platform adapter; the widget owns the
/// resolve intent and reads the current projection each frame.
///
/// ADR-0063 (#1671): `resolve_ref` replaces the old `claim_profile` — the
/// host calls a typed profile-ref adapter and reads the resolved row from the shell's
/// `RefProfileStore` mirror, not from the old `claimed_profiles` map.
pub trait NostrMentionProfileHost {
    fn profile_for_pubkey(&self, pubkey: &str) -> Option<ContentProfileRenderData>;
    /// Issue a typed profile-ref resolve to the kernel. Called by the widget on every render frame for
    /// each visible pubkey; the kernel deduplicates and refcounts.
    fn resolve_ref(&self, pubkey: &str, consumer_id: &str);
}

/// Inline terminal chip for a profile mention.
pub struct NostrMentionChip<'a> {
    uri: &'a WireUri,
    profile: Option<&'a ContentProfileRenderData>,
    profile_host: Option<&'a dyn NostrMentionProfileHost>,
    consumer_id: Option<&'a str>,
    style: Style,
}

impl<'a> NostrMentionChip<'a> {
    pub fn new(uri: &'a WireUri) -> Self {
        Self {
            uri,
            profile: None,
            profile_host: None,
            consumer_id: None,
            style: Style::default()
                .fg(mention_color(&uri.primary_id))
                .add_modifier(Modifier::BOLD),
        }
    }

    pub fn profile(mut self, profile: Option<&'a ContentProfileRenderData>) -> Self {
        self.profile = profile;
        self
    }

    pub fn profile_host(mut self, host: Option<&'a dyn NostrMentionProfileHost>) -> Self {
        self.profile_host = host;
        self
    }

    pub fn consumer_id(mut self, id: Option<&'a str>) -> Self {
        self.consumer_id = id;
        self
    }

    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub fn label(&self) -> String {
        let resolved = self.resolved_profile();
        let profile = self.profile.or(resolved.as_ref());
        let raw = profile
            .map(ContentProfileRenderData::label)
            .unwrap_or(&self.uri.primary_id);
        let label = self
            .profile
            .or(profile)
            .and_then(|profile| profile.display_name.as_deref())
            .map(str::to_string)
            .unwrap_or_else(|| short_id(raw));
        format!("@{label}")
    }

    pub fn span(&self) -> Span<'static> {
        Span::styled(self.label(), self.style)
    }

    fn resolved_profile(&self) -> Option<ContentProfileRenderData> {
        if !is_hex_id_64(&self.uri.primary_id) {
            return None;
        }
        let host = self.profile_host?;
        if let Some(consumer_id) = self.consumer_id {
            host.resolve_ref(&self.uri.primary_id, consumer_id);
        }
        host.profile_for_pubkey(&self.uri.primary_id)
    }
}

impl Widget for NostrMentionChip<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(Line::from(self.span())).render(area, buf);
    }
}

fn short_id(id: &str) -> String {
    let count = id.chars().count();
    if count <= 12 {
        id.to_string()
    } else {
        let head = id.chars().take(6).collect::<String>();
        let tail = id
            .chars()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>();
        format!("{head}…{tail}")
    }
}

fn is_hex_id_64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn mention_color(id: &str) -> Color {
    let hue = stable_hash(id) % 360;
    let (r, g, b) = hsl_to_rgb(hue as f32, 0.72, 0.66);
    Color::Rgb(r, g, b)
}

fn stable_hash(value: &str) -> u32 {
    value.bytes().fold(5381u32, |hash, byte| {
        hash.wrapping_mul(33) ^ u32::from(byte)
    })
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h_prime = h / 60.0;
    let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match h_prime {
        h if (0.0..1.0).contains(&h) => (c, x, 0.0),
        h if (1.0..2.0).contains(&h) => (x, c, 0.0),
        h if (2.0..3.0).contains(&h) => (0.0, c, x),
        h if (3.0..4.0).contains(&h) => (0.0, x, c),
        h if (4.0..5.0).contains(&h) => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    (channel(r1 + m), channel(g1 + m), channel(b1 + m))
}

fn channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[test]
    fn mention_color_is_deterministic_per_pubkey() {
        const SHOWCASE_PUBKEY: &str =
            "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52";
        const SHOWCASE_NOTE_ID: &str =
            "276d69d6d2dc8348d2a0b7a67245503909dc5a405d7bae61a824dc224e11d784";
        let first = mention_color(SHOWCASE_PUBKEY);
        let second = mention_color(SHOWCASE_PUBKEY);
        let other = mention_color(SHOWCASE_NOTE_ID);
        assert_eq!(first, second);
        assert_ne!(first, other);
    }

    #[test]
    fn mention_label_resolves_ref_and_reads_host_projection() {
        struct Host {
            claimed: RefCell<Vec<(String, String)>>,
        }

        impl NostrMentionProfileHost for Host {
            fn profile_for_pubkey(&self, pubkey: &str) -> Option<ContentProfileRenderData> {
                Some(ContentProfileRenderData {
                    pubkey: pubkey.to_string(),
                    display_name: Some("pablof7z".to_string()),
                    npub: None,
                    picture_url: None,
                })
            }

            fn resolve_ref(&self, pubkey: &str, consumer_id: &str) {
                self.claimed
                    .borrow_mut()
                    .push((pubkey.to_string(), consumer_id.to_string()));
            }
        }

        let host = Host {
            claimed: RefCell::new(Vec::new()),
        };
        let uri = WireUri {
            uri: "nostr:profile".to_string(),
            kind: "npub".to_string(),
            primary_id: "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52"
                .to_string(),
            relays: Vec::new(),
            author: None,
            event_kind: None,
        };

        let label = NostrMentionChip::new(&uri)
            .profile_host(Some(&host))
            .consumer_id(Some("content-mention-chip"))
            .label();

        assert_eq!(label, "@pablof7z");
        assert_eq!(
            host.claimed.borrow().as_slice(),
            [(
                "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52".to_string(),
                "content-mention-chip".to_string()
            )]
        );
    }
}
