//! Approach-b Chats tab: 2-pane DM view.
//!
//! Left pane (38%): conversation list with 2-row cards.
//! Right pane (62%): message transcript + optional inline compose strip.
//!
//! Compose strip fields (`chat_composing`, `chat_compose_buf`) are
//! plumbed through file-local helpers that return stub values until the
//! companion wiring PR adds them to `AppState`.
//!
//! Per-message timestamps are omitted: `MessageLine` does not yet expose
//! `created_at`. This will be addressed when the snapshot adds that field.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::AppState;
use crate::feature_snapshot::{DmConversationLine, MessageLine};
use crate::ui::colors::{
    ACCENT_CYAN, BODY_TEXT, DETAIL_BG, DIM_TEXT, DIMMER_TEXT, LIST_BG, SELECTED_BG, author_color,
};

// ---------------------------------------------------------------------------
// Stub helpers — replaced by the wiring agent when AppState fields land.
// ---------------------------------------------------------------------------

/// Whether the inline DM compose strip is open.
fn is_composing(state: &AppState) -> bool {
    state.chat_composing
}

/// Current text in the DM compose buffer.
fn compose_buf(state: &AppState) -> &str {
    &state.chat_compose_buf
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(area);

    render_conversation_list(frame, cols[0], state);
    render_message_thread(frame, cols[1], state);
}

// ---------------------------------------------------------------------------
// Left pane: conversation list
// ---------------------------------------------------------------------------

fn render_conversation_list(frame: &mut Frame, area: Rect, state: &AppState) {
    let focused = true; // TODO(wiring): derive focus from AppState.focused pane enum
    let border_color = if focused { ACCENT_CYAN } else { DIMMER_TEXT };

    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(LIST_BG))
        .title(Span::styled(
            " Chats ",
            Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    let pane_width = inner.width as usize;

    let lines = if state.features.dm_conversations.is_empty() {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No conversations yet  \u{00b7}  n to start a new DM",
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

    let avatar_color = author_color(&conv.peer_pubkey);

    // Row 1: avatar (2 block chars) + space + peer display name (bold, author-colored)
    // No timestamp available from the current snapshot.
    let avatar_span = Span::styled(
        "\u{2588}\u{2588} ",
        Style::default().fg(avatar_color).bg(row_bg),
    );
    // avatar (2) + space (1) = 3 chars consumed
    let name_max = content_width.saturating_sub(3);
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

    // Row 2: indented last-message preview (dim)
    let indent = "  ";
    let indent_len = indent.chars().count();
    let preview_max = content_width.saturating_sub(indent_len);
    let preview = truncate(conv.latest.replace('\n', " ").as_str(), preview_max);
    let preview_len = preview.chars().count();
    let pad2_len = content_width.saturating_sub(indent_len + preview_len);
    lines.push(Line::from(vec![
        gutter,
        Span::styled(indent, Style::default().bg(row_bg)),
        Span::styled(preview, Style::default().fg(DIM_TEXT).bg(row_bg)),
        Span::styled(" ".repeat(pad2_len), Style::default().bg(row_bg)),
    ]));
}

// ---------------------------------------------------------------------------
// Right pane: transcript + optional inline compose strip
// ---------------------------------------------------------------------------

fn render_message_thread(frame: &mut Frame, area: Rect, state: &AppState) {
    let composing = is_composing(state);

    // When composing, split the pane: messages above, 3-row compose strip below.
    let (msg_area, compose_area_opt) = if composing && area.height > 3 {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(3),
            ])
            .split(area);
        (chunks[0], Some(chunks[1]))
    } else {
        (area, None)
    };

    // Transcript block
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT_CYAN))
        .style(Style::default().bg(DETAIL_BG))
        .title(Span::styled(
            " Messages ",
            Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(msg_area);
    let pane_width = inner.width as usize;

    let conv = state.features.dm_conversations.get(state.chat_selected);

    let lines = match conv {
        None => empty_state("  No messages yet"),
        Some(c) if c.messages.is_empty() => empty_state("  No messages yet"),
        Some(c) => build_message_lines(&c.messages, pane_width),
    };

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(DETAIL_BG).fg(BODY_TEXT));
    frame.render_widget(paragraph, msg_area);

    // Inline compose strip
    if let Some(compose_area) = compose_area_opt {
        let peer = state
            .features
            .dm_conversations
            .get(state.chat_selected)
            .map(|c| c.peer_display.as_str())
            .unwrap_or("?");
        render_compose_strip(frame, compose_area, state, peer);
    }
}

fn build_message_lines(messages: &[MessageLine], pane_width: usize) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    for msg in messages.iter().take(30) {
        if msg.outgoing {
            // Right-aligned outgoing:  "              you  \u{203a}  content"
            let prefix = "you  \u{203a}  ";
            let max_body = pane_width.saturating_sub(prefix.len() + 1);
            let body = truncate(&msg.content.replace('\n', " "), max_body);
            let body_len = body.chars().count();
            let total_len = prefix.len() + body_len;
            let left_pad = pane_width.saturating_sub(total_len);
            out.push(Line::from(vec![
                Span::raw(" ".repeat(left_pad)),
                Span::styled(prefix, Style::default().fg(ACCENT_CYAN)),
                Span::styled(body, Style::default().fg(BODY_TEXT)),
            ]));
        } else {
            // Left-aligned incoming:  "@short_name  \u{203a}  content"
            let author_col = author_color(&msg.author);
            let short = short_author(&msg.author);
            let label = format!("@{short}  \u{203a}  ");
            let label_len = label.chars().count();
            let max_body = pane_width.saturating_sub(label_len);
            let body = truncate(&msg.content.replace('\n', " "), max_body);
            out.push(Line::from(vec![
                Span::styled(label, Style::default().fg(author_col)),
                Span::styled(body, Style::default().fg(BODY_TEXT)),
            ]));
        }
        out.push(Line::from(""));
    }
    out
}

fn render_compose_strip(frame: &mut Frame, area: Rect, state: &AppState, peer: &str) {
    let buf = compose_buf(state);
    let inner_width = area.width.saturating_sub(4) as usize;
    let header = truncate(&format!(" Compose to @{peer} "), inner_width);
    let body = truncate(&format!("> {buf}\u{2588}"), inner_width);
    let footer = truncate(
        " Ctrl+Enter send  Esc cancel  Enter newline ",
        inner_width,
    );

    let lines = vec![
        Line::from(vec![
            Span::styled(
                format!("\u{251c}\u{2500} {header}"),
                Style::default().fg(ACCENT_CYAN),
            ),
        ]),
        Line::from(vec![
            Span::styled("\u{2502} ", Style::default().fg(ACCENT_CYAN)),
            Span::styled(body, Style::default().fg(BODY_TEXT)),
        ]),
        Line::from(vec![
            Span::styled(
                format!("\u{2514}\u{2500} {footer}"),
                Style::default().fg(ACCENT_CYAN),
            ),
        ]),
    ];

    let paragraph = Paragraph::new(lines)
        .style(Style::default().bg(DETAIL_BG));
    frame.render_widget(paragraph, area);
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

fn empty_state(msg: &'static str) -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        Line::from(Span::styled(msg, Style::default().fg(DIM_TEXT))),
    ]
}

fn short_author(value: &str) -> String {
    const MAX: usize = 12;
    if value.chars().count() <= MAX {
        value.to_string()
    } else {
        let prefix: String = value.chars().take(6).collect();
        let suffix: String = value.chars().rev().take(4).collect::<String>().chars().rev().collect();
        format!("{prefix}..{suffix}")
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
