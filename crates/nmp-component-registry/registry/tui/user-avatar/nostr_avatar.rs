use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};
use ratatui_image::{protocol::Protocol, Image};

use super::profile_wire::ProfileWire;

/// Host bridge for profile projections owned by the NMP kernel.
///
/// Immediate-mode TUI widgets call this while rendering visible profile
/// references. The host supplies the platform adapter; the widget owns the
/// resolve intent and reads the current projection each frame.
///
/// ADR-0063 (#1671): `resolve_ref` replaces the old `claim_profile` — the
/// host calls a typed profile-ref adapter and reads the resolved row from the shell's
/// `RefProfileStore` mirror, not from the old `claimed_profiles` map.
pub trait NostrProfileHost {
    fn profile_for_pubkey(&self, pubkey: &str) -> ProfileWire;
    /// Issue a typed profile-ref resolve to the kernel. Called by the widget on every render frame for
    /// each visible pubkey; the kernel deduplicates and refcounts.
    fn resolve_ref(&self, pubkey: &str, consumer_id: &str);
    fn release_ref(&self, pubkey: &str, consumer_id: &str);
}

const PALETTE: [Color; 8] = [
    Color::Rgb(244, 114, 182),
    Color::Rgb(56, 189, 248),
    Color::Rgb(52, 211, 153),
    Color::Rgb(251, 191, 36),
    Color::Rgb(167, 139, 250),
    Color::Rgb(248, 113, 113),
    Color::Rgb(45, 212, 191),
    Color::Rgb(250, 204, 21),
];

/// Circular-ish terminal avatar with deterministic identicon fallback.
///
/// Terminals render cells, not real circles, so this widget uses a compact
/// bordered tile with profile initials and a stable pubkey-derived accent.
pub struct NostrAvatar<'a> {
    profile: AvatarProfile<'a>,
    image: Option<&'a Protocol>,
    border_style: Style,
}

enum AvatarProfile<'a> {
    Borrowed(&'a ProfileWire),
    Owned(ProfileWire),
}

impl<'a> NostrAvatar<'a> {
    pub fn new(profile: &'a ProfileWire) -> Self {
        Self {
            profile: AvatarProfile::Borrowed(profile),
            image: None,
            border_style: Style::default().fg(accent_for(&profile.pubkey)),
        }
    }

    pub fn for_pubkey(pubkey: &str, host: &dyn NostrProfileHost) -> Self {
        const CONSUMER_ID: &str = "tui/user-avatar";
        host.resolve_ref(pubkey, CONSUMER_ID);
        let profile = host.profile_for_pubkey(pubkey);
        let border_style = Style::default().fg(accent_for(&profile.pubkey));
        Self {
            profile: AvatarProfile::Owned(profile),
            image: None,
            border_style,
        }
    }

    pub fn image(mut self, image: Option<&'a Protocol>) -> Self {
        self.image = image;
        self
    }

    pub fn border_style(mut self, style: Style) -> Self {
        self.border_style = style;
        self
    }

    fn profile(&self) -> &ProfileWire {
        match &self.profile {
            AvatarProfile::Borrowed(profile) => profile,
            AvatarProfile::Owned(profile) => profile,
        }
    }
}

impl Widget for NostrAvatar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let profile = self.profile();
        let accent = accent_for(&profile.pubkey);
        let initials = profile.initials();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(self.border_style)
            .style(Style::default().bg(Color::Reset));
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.is_empty() {
            return;
        }
        if let Some(image) = self.image {
            Image::new(image).allow_clipping(true).render(inner, buf);
            return;
        }

        let fill = Style::default()
            .fg(Color::Black)
            .bg(accent)
            .add_modifier(Modifier::BOLD);
        let line = Line::from(Span::styled(initials, fill));
        Paragraph::new(line)
            .alignment(Alignment::Center)
            .style(fill)
            .render(center_line(inner), buf);
    }
}

fn accent_for(pubkey: &str) -> Color {
    let hash = pubkey.bytes().fold(5381usize, |acc, byte| {
        ((acc << 5).wrapping_add(acc)) ^ byte as usize
    });
    PALETTE[hash % PALETTE.len()]
}

fn center_line(area: Rect) -> Rect {
    Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(1) / 2,
        width: area.width,
        height: 1,
    }
}
