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
    let pane_width = inner.width as usize;

    let lines = if state.features.dm_conversations.is_empty() {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No conversations yet",
                Style::default().fg(DIM_TEXT),
            )),
        ]
    } else {
        let mut all: Vec<Line<'static>> = Vec::new();
        for (i, conv) in state.features.dm_conversations.iter().enumerate() {
            let selected = i == state.chat_selected;
            append_conversation_card(&mut all, conv, selected, pane_width);
        }
        all
    };

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(LIST_BG).fg(BODY_TEXT));
    frame.render_widget(paragraph, area);
}

fn append_conversation_card(
    lines: &mut Vec<Line<'static>>,
    conv: &DmConversationLine,
    selected: bool,
    pane_width: usize,
) {
    let row_bg = if selected { SELECTED_BG } else { LIST_BG };
    let gutter = if selected {
        Span::styled("\u{2503} ", Style::default().fg(ACCENT_CYAN).bg(row_bg))
    } else {
        Span::styled("  ", Style::default().bg(row_bg))
    };
    let gutter_width = 2usize;
    let content_width = pane_width.saturating_sub(gutter_width);

    // Row 1: avatar block + peer name
    let avatar_color = author_color(&conv.peer_pubkey);
    let avatar_span = Span::styled(
        "\u{2588}\u{2588} ",
        Style::default().fg(avatar_color).bg(row_bg),
    );
    let name_max = content_width.saturating_sub(4);
    let name = truncate(&conv.peer_display, name_max);
    let name_len = name.chars().count();
    let name_span = Span::styled(
        name,
        Style::default()
            .fg(avatar_color)
            .bg(row_bg)
            .add_modifier(Modifier::BOLD),
    );
    let pad1_len = content_width.saturating_sub(3 + name_len);
    let pad1 = Span::styled(" ".repeat(pad1_len), Style::default().bg(row_bg));
    lines.push(Line::from(vec![
        gutter.clone(),
        avatar_span,
        name_span,
        pad1,
    ]));

    // Row 2: last message preview
    let preview = truncate(conv.latest.replace('\n', " ").as_str(), content_width);
    let preview_len = preview.chars().count();
    let pad2_len = content_width.saturating_sub(preview_len);
    lines.push(Line::from(vec![
        gutter,
        Span::styled(preview, Style::default().fg(DIM_TEXT).bg(row_bg)),
        Span::styled(" ".repeat(pad2_len), Style::default().bg(row_bg)),
    ]));
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
