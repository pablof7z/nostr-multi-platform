use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::components::nostr_user::{
    nostr_avatar::{NostrAvatar, NostrProfileHost},
    nostr_profile_name::NostrProfileName,
    profile_wire::ProfileWire,
};

use super::nostr_group_chat_wire::NostrGroupChatParticipantWire;

const AVATAR_WIDTH: u16 = 6;
const ROW_HEIGHT: u16 = 3;

/// Ratatui group roster list over Rust-owned participant rows.
///
/// Selection, context menus, and moderation/admin actions stay in the host app.
pub struct NostrGroupRosterList<'a> {
    participants: &'a [NostrGroupChatParticipantWire],
    profile_host: Option<&'a dyn NostrProfileHost>,
}

impl<'a> NostrGroupRosterList<'a> {
    pub fn new(participants: &'a [NostrGroupChatParticipantWire]) -> Self {
        Self {
            participants,
            profile_host: None,
        }
    }

    pub fn profile_host(mut self, host: Option<&'a dyn NostrProfileHost>) -> Self {
        self.profile_host = host;
        self
    }

    pub fn preferred_height(&self) -> u16 {
        self.participants
            .len()
            .saturating_mul(ROW_HEIGHT as usize)
            .min(u16::MAX as usize) as u16
    }

    fn profile_for(&self, participant: &NostrGroupChatParticipantWire) -> ProfileWire {
        if let Some(host) = self.profile_host {
            let consumer_id = format!("chat.roster.{}.profile", participant.pubkey);
            host.resolve_ref(&participant.pubkey, &consumer_id);
            return host.profile_for_pubkey(&participant.pubkey);
        }
        fallback_profile(&participant.pubkey)
    }
}

impl Widget for NostrGroupRosterList<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        for (idx, participant) in self.participants.iter().enumerate() {
            let y = area
                .y
                .saturating_add((idx as u16).saturating_mul(ROW_HEIGHT));
            if y >= area.y.saturating_add(area.height) {
                break;
            }
            let row = Rect {
                x: area.x,
                y,
                width: area.width,
                height: ROW_HEIGHT.min(area.y.saturating_add(area.height).saturating_sub(y)),
            };
            let profile = self.profile_for(participant);
            if row.width > AVATAR_WIDTH {
                NostrAvatar::new(&profile).render(
                    Rect {
                        x: row.x,
                        y: row.y,
                        width: AVATAR_WIDTH.saturating_sub(1),
                        height: row.height,
                    },
                    buf,
                );
            }

            let text_area = Rect {
                x: row.x + AVATAR_WIDTH.min(row.width),
                y: row.y,
                width: row.width.saturating_sub(AVATAR_WIDTH),
                height: row.height,
            };
            Paragraph::new(lines_for(participant, &profile)).render(text_area, buf);
        }
    }
}

fn lines_for(
    participant: &NostrGroupChatParticipantWire,
    profile: &ProfileWire,
) -> Vec<Line<'static>> {
    let name = NostrProfileName::new(profile)
        .style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .line();
    let mut meta = Vec::new();
    if let Some(role) = participant.role_label() {
        meta.push(Span::styled(
            role.to_string(),
            Style::default().fg(Color::Cyan),
        ));
    }
    if let Some(status) = participant.status_label() {
        if !meta.is_empty() {
            meta.push(Span::raw("  "));
        }
        meta.push(Span::styled(
            status.to_string(),
            Style::default().fg(Color::DarkGray),
        ));
    }
    vec![name, Line::from(meta)]
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
