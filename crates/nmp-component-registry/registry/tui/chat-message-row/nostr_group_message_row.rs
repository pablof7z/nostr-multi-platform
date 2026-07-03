use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
};

use crate::components::nostr_user::{
    nostr_avatar::{NostrAvatar, NostrProfileHost},
    nostr_profile_name::NostrProfileName,
    profile_wire::ProfileWire,
};

use super::nostr_group_chat_wire::NostrGroupChatMessageWire;

const AVATAR_WIDTH: u16 = 6;
const MAX_BUBBLE_WIDTH: u16 = 72;

/// Ratatui group-chat message row.
///
/// This widget renders the Rust-projected row and optionally self-claims the
/// author's profile through the existing TUI user component host. Reply taps,
/// message menus, and send actions stay in the app input loop.
pub struct NostrGroupMessageRow<'a> {
    message: &'a NostrGroupChatMessageWire,
    profile_host: Option<&'a dyn NostrProfileHost>,
    author_profile: Option<&'a ProfileWire>,
    max_width: u16,
}

impl<'a> NostrGroupMessageRow<'a> {
    pub fn new(message: &'a NostrGroupChatMessageWire) -> Self {
        Self {
            message,
            profile_host: None,
            author_profile: None,
            max_width: MAX_BUBBLE_WIDTH,
        }
    }

    pub fn profile_host(mut self, host: Option<&'a dyn NostrProfileHost>) -> Self {
        self.profile_host = host;
        self
    }

    pub fn author_profile(mut self, profile: Option<&'a ProfileWire>) -> Self {
        self.author_profile = profile;
        self
    }

    pub fn max_width(mut self, width: u16) -> Self {
        self.max_width = width.max(1);
        self
    }

    pub fn preferred_height(&self, width: u16) -> u16 {
        let content_width = self.text_width(width).max(1) as usize;
        let mut height = if self.message.is_outgoing { 0 } else { 1 };
        if let Some(reply) = self.message.reply_preview() {
            height += wrapped_line_count(reply, content_width).max(1);
        }
        height += wrapped_line_count(&self.message.content, content_width).max(1);
        if !self.message.reactions.is_empty() {
            height += 1;
        }
        if self.message.is_outgoing {
            height.max(1)
        } else {
            height.max(3)
        }
    }

    fn text_width(&self, width: u16) -> u16 {
        let available = if self.message.is_outgoing {
            width
        } else {
            width.saturating_sub(AVATAR_WIDTH)
        };
        available.min(self.max_width).max(1)
    }

    fn resolved_author_profile(&self) -> ProfileWire {
        if let Some(profile) = self.author_profile {
            return profile.clone();
        }
        if let Some(host) = self.profile_host {
            let consumer_id = format!("chat.message.{}.profile", self.message.id);
            host.resolve_ref(&self.message.author_pubkey, &consumer_id);
            return host.profile_for_pubkey(&self.message.author_pubkey);
        }
        fallback_profile(&self.message.author_pubkey)
    }

    fn lines(&self, profile: &ProfileWire) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        if !self.message.is_outgoing {
            let mut header = NostrProfileName::new(profile)
                .style(author_style(&profile.pubkey))
                .line()
                .spans;
            header.push(Span::raw("  "));
            header.push(Span::styled(
                self.message.created_at_label.clone(),
                Style::default().fg(Color::DarkGray),
            ));
            lines.push(Line::from(header));
        }

        if let Some(reply) = self.message.reply_preview() {
            lines.push(Line::from(Span::styled(
                format!("> {reply}"),
                Style::default().fg(Color::DarkGray),
            )));
        }

        let body_style = if self.message.is_outgoing {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::White).bg(Color::Rgb(31, 41, 55))
        };
        lines.push(Line::from(Span::styled(
            format!(" {} ", self.message.content),
            body_style,
        )));

        if !self.message.reactions.is_empty() {
            let spans = self
                .message
                .reactions
                .iter()
                .enumerate()
                .flat_map(|(idx, reaction)| {
                    let prefix = (idx > 0).then_some(Span::raw(" "));
                    [
                        prefix,
                        Some(Span::styled(
                            format!(" {} ", reaction.label()),
                            Style::default()
                                .fg(Color::LightYellow)
                                .bg(Color::Rgb(55, 65, 81)),
                        )),
                    ]
                    .into_iter()
                    .flatten()
                })
                .collect::<Vec<_>>();
            lines.push(Line::from(spans));
        }

        lines
    }
}

impl Widget for NostrGroupMessageRow<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let profile = self.resolved_author_profile();
        let text_width = self.text_width(area.width);
        let text_x = if self.message.is_outgoing {
            area.x + area.width.saturating_sub(text_width)
        } else {
            area.x + AVATAR_WIDTH.min(area.width)
        };
        let text_area = Rect {
            x: text_x,
            y: area.y,
            width: text_width.min(area.width),
            height: area.height,
        };

        if !self.message.is_outgoing && area.width > AVATAR_WIDTH {
            let avatar = Rect {
                x: area.x,
                y: area.y,
                width: AVATAR_WIDTH.saturating_sub(1),
                height: area.height.min(3),
            };
            NostrAvatar::new(&profile).render(avatar, buf);
        }

        Paragraph::new(self.lines(&profile))
            .wrap(Wrap { trim: false })
            .render(text_area, buf);
    }
}

fn fallback_profile(pubkey: &str) -> ProfileWire {
    ProfileWire {
        pubkey: pubkey.to_string(),
        display_name: None,
        about: None,
        picture_url: None,
        nip05: None,
        npub: pubkey.to_string(),
        npub_short: short_pubkey(pubkey),
    }
}

fn author_style(pubkey: &str) -> Style {
    Style::default()
        .fg(author_color(pubkey))
        .add_modifier(Modifier::BOLD)
}

fn author_color(pubkey: &str) -> Color {
    const COLORS: [Color; 6] = [
        Color::LightBlue,
        Color::LightCyan,
        Color::LightGreen,
        Color::LightMagenta,
        Color::LightRed,
        Color::Yellow,
    ];
    let hash = pubkey.bytes().fold(5381usize, |acc, byte| {
        ((acc << 5).wrapping_add(acc)) ^ byte as usize
    });
    COLORS[hash % COLORS.len()]
}

fn wrapped_line_count(value: &str, width: usize) -> u16 {
    let width = width.max(1);
    value
        .lines()
        .map(|line| line.chars().count().max(1).div_ceil(width))
        .sum::<usize>()
        .max(1)
        .min(u16::MAX as usize) as u16
}

fn short_pubkey(pubkey: &str) -> String {
    let count = pubkey.chars().count();
    if count <= 12 {
        pubkey.to_string()
    } else {
        let head = pubkey.chars().take(6).collect::<String>();
        let tail = pubkey
            .chars()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>();
        format!("{head}...{tail}")
    }
}
