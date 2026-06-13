//! Approach-b Chats tab: 2-pane split showing DM conversation list + message thread.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::AppState;
use crate::feature_snapshot::{DmConversationLine, MessageLine};
use crate::ui::colors::{
    author_color, ACCENT_CYAN, BODY_TEXT, DETAIL_BG, DIM_TEXT, LIST_BG, SELECTED_BG,
};
use crate::ui::nostr_user::nostr_avatar::NostrAvatar;
use crate::ui::nostr_user::profile_wire::ProfileWire;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(area);

    render_conversation_list(frame, cols[0], state);
    render_message_thread(frame, cols[1], state);
}

fn render_conversation_list(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(ACCENT_CYAN))
        .style(Style::default().bg(LIST_BG))
        .title(Span::styled(
            " Chats ",
            Style::default()
                .fg(ACCENT_CYAN)
                .add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if state.features.dm_conversations.is_empty() {
        let placeholder = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No conversations yet",
                Style::default().fg(DIM_TEXT),
            )),
        ])
        .style(Style::default().bg(LIST_BG).fg(BODY_TEXT));
        frame.render_widget(placeholder, inner);
        return;
    }

    // Each conversation card is 3 rows (3 = minimum for NostrAvatar with
    // borders to show initials: top-border + initials + bottom-border).
    const CARD_HEIGHT: u16 = 3;
    let mut card_y = inner.y;
    for (i, conv) in state.features.dm_conversations.iter().enumerate() {
        if card_y >= inner.y + inner.height {
            break;
        }
        let card_height = CARD_HEIGHT.min((inner.y + inner.height).saturating_sub(card_y));
        let card_rect = Rect {
            x: inner.x,
            y: card_y,
            width: inner.width,
            height: card_height,
        };
        let selected = i == state.chat_selected;
        render_conversation_card(frame, card_rect, conv, selected);
        card_y += CARD_HEIGHT;
    }
}

fn render_conversation_card(
    frame: &mut Frame,
    area: Rect,
    conv: &DmConversationLine,
    selected: bool,
) {
    let row_bg = if selected { SELECTED_BG } else { LIST_BG };

    let gutter_width = 2u16;
    let avatar_width = 4u16; // 2 border cols + 2 interior cols for initials
    let text_x = area.x + gutter_width + avatar_width;
    let text_width = area.width.saturating_sub(gutter_width + avatar_width);

    // Gutter (selection indicator).
    let gutter_rect = Rect {
        x: area.x,
        y: area.y,
        width: gutter_width,
        height: area.height,
    };
    let gutter_line = if selected {
        Paragraph::new(vec![
            Line::from(Span::styled(
                "\u{2503} ",
                Style::default().fg(ACCENT_CYAN).bg(row_bg),
            )),
            Line::from(Span::styled(
                "\u{2503} ",
                Style::default().fg(ACCENT_CYAN).bg(row_bg),
            )),
            Line::from(Span::styled(
                "\u{2503} ",
                Style::default().fg(ACCENT_CYAN).bg(row_bg),
            )),
        ])
    } else {
        Paragraph::new(vec![
            Line::from(Span::styled("  ", Style::default().bg(row_bg))),
            Line::from(Span::styled("  ", Style::default().bg(row_bg))),
            Line::from(Span::styled("  ", Style::default().bg(row_bg))),
        ])
    };
    frame.render_widget(gutter_line, gutter_rect);

    // NostrAvatar — registry component; minimal ProfileWire from conversation data.
    let peer_wire = ProfileWire {
        pubkey: conv.peer_pubkey.clone(),
        display_name: Some(conv.peer_display.clone()),
        about: None,
        picture_url: None,
        nip05: None,
        npub: conv.peer_pubkey.clone(),
        npub_short: short_author(&conv.peer_pubkey),
    };
    let avatar_rect = Rect {
        x: area.x + gutter_width,
        y: area.y,
        width: avatar_width,
        height: area.height,
    };
    frame.render_widget(NostrAvatar::new(&peer_wire).image(None), avatar_rect);

    // Text: row 0 = peer name, row 1 = message preview, row 2 = padding.
    if text_width == 0 {
        return;
    }
    let name_max = text_width as usize;
    let name = truncate(&conv.peer_display, name_max);
    let name_len = name.chars().count();
    let name_pad = " ".repeat(name_max.saturating_sub(name_len));
    let preview_str = truncate(conv.latest.replace('\n', " ").as_str(), text_width as usize);
    let preview_len = preview_str.chars().count();
    let preview_pad = " ".repeat((text_width as usize).saturating_sub(preview_len));
    let text_paragraph = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                name,
                Style::default()
                    .fg(author_color(&conv.peer_pubkey))
                    .bg(row_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(name_pad, Style::default().bg(row_bg)),
        ]),
        Line::from(vec![
            Span::styled(preview_str, Style::default().fg(DIM_TEXT).bg(row_bg)),
            Span::styled(preview_pad, Style::default().bg(row_bg)),
        ]),
        Line::from(Span::styled(
            " ".repeat(text_width as usize),
            Style::default().bg(row_bg),
        )),
    ])
    .style(Style::default().bg(row_bg));
    frame.render_widget(
        text_paragraph,
        Rect {
            x: text_x,
            y: area.y,
            width: text_width,
            height: area.height,
        },
    );
}

fn render_message_thread(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT_CYAN))
        .style(Style::default().bg(DETAIL_BG))
        .title(Span::styled(
            " Messages ",
            Style::default()
                .fg(ACCENT_CYAN)
                .add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    let pane_width = inner.width as usize;

    let conv = state.features.dm_conversations.get(state.chat_selected);

    let lines = match conv {
        None => vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No messages yet",
                Style::default().fg(DIM_TEXT),
            )),
        ],
        Some(c) if c.messages.is_empty() => vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No messages yet",
                Style::default().fg(DIM_TEXT),
            )),
        ],
        Some(c) => build_message_lines(&c.messages, pane_width),
    };

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(DETAIL_BG).fg(BODY_TEXT));
    frame.render_widget(paragraph, area);
}

fn build_message_lines(messages: &[MessageLine], pane_width: usize) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    for msg in messages.iter().take(20) {
        if msg.outgoing {
            // Right-aligned outgoing message
            let prefix = "you ";
            let max_body = pane_width.saturating_sub(prefix.len() + 2);
            let body = truncate(&msg.content.replace('\n', " "), max_body);
            let body_len = body.chars().count();
            let total_len = prefix.len() + body_len;
            let left_pad = pane_width.saturating_sub(total_len);
            out.push(Line::from(vec![
                Span::raw(" ".repeat(left_pad)),
                Span::styled(prefix, Style::default().fg(DIM_TEXT)),
                Span::styled(body, Style::default().fg(ACCENT_CYAN)),
            ]));
        } else {
            // Left-aligned incoming message
            let author_col = author_color(&msg.author);
            let short_author = short_author(&msg.author);
            let author_label = format!("{short_author}: ");
            let max_body = pane_width.saturating_sub(author_label.chars().count());
            let body = truncate(&msg.content.replace('\n', " "), max_body);
            out.push(Line::from(vec![
                Span::styled(author_label, Style::default().fg(author_col)),
                Span::styled(body, Style::default().fg(BODY_TEXT)),
            ]));
        }
        // Blank separator between messages
        out.push(Line::from(""));
    }
    out
}

fn short_author(value: &str) -> String {
    if value.len() <= 12 {
        value.to_string()
    } else {
        format!(
            "{}..{}",
            &value[..6],
            &value[value.len().saturating_sub(4)..]
        )
    }
}

fn truncate(value: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let count = value.chars().count();
    if count <= max {
        value.to_string()
    } else if max <= 1 {
        value.chars().take(max).collect()
    } else {
        let mut out: String = value.chars().take(max.saturating_sub(1)).collect();
        out.push('\u{2026}');
        out
    }
}
