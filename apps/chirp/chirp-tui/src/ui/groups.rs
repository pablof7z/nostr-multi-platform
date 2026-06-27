//! Approach-b Groups tab: 2-pane split showing group list + group chat.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::AppState;
use crate::feature_snapshot::{GroupLine, MessageLine};
use crate::ui::colors::{
    author_color, ACCENT_CYAN, BODY_TEXT, DETAIL_BG, DIM_TEXT, LIST_BG, REPOST, SELECTED_BG, ZAP,
};

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(area);

    render_group_list(frame, cols[0], state);
    render_group_events(frame, cols[1], state);
}

fn render_group_list(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(ACCENT_CYAN))
        .style(Style::default().bg(LIST_BG))
        .title(Span::styled(
            " Groups ",
            Style::default()
                .fg(ACCENT_CYAN)
                .add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    let pane_width = inner.width as usize;

    let lines = if state.features.discovered_groups.is_empty() {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No groups discovered yet",
                Style::default().fg(DIM_TEXT),
            )),
        ]
    } else {
        let mut all: Vec<Line<'static>> = Vec::new();
        for (i, group) in state.features.discovered_groups.iter().enumerate() {
            let selected = i == state.group_selected;
            append_group_card(&mut all, group, selected, pane_width);
        }
        all
    };

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(LIST_BG).fg(BODY_TEXT));
    frame.render_widget(paragraph, area);
}

fn append_group_card(
    lines: &mut Vec<Line<'static>>,
    group: &GroupLine,
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

    // Row 1: group name + member count + open/closed badge
    let badge = if group.open { "[open]" } else { "[closed]" };
    let badge_color = if group.open { REPOST } else { ZAP };
    let members_str = format!(" {} members ", group.member_count);
    let right_part_len = members_str.chars().count() + badge.chars().count();
    let name_max = content_width.saturating_sub(right_part_len + 1);
    let name = truncate(&group.name, name_max);
    let name_len = name.chars().count();
    let mid_pad_len = content_width.saturating_sub(name_len + right_part_len);

    lines.push(Line::from(vec![
        gutter.clone(),
        Span::styled(
            name,
            Style::default()
                .fg(ACCENT_CYAN)
                .bg(row_bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ".repeat(mid_pad_len), Style::default().bg(row_bg)),
        Span::styled(members_str, Style::default().fg(DIM_TEXT).bg(row_bg)),
        Span::styled(badge, Style::default().fg(badge_color).bg(row_bg)),
    ]));

    // Row 2: relay URL dim
    let relay = truncate(&group.host_relay_url, content_width);
    let relay_len = relay.chars().count();
    let pad2_len = content_width.saturating_sub(relay_len);
    lines.push(Line::from(vec![
        gutter,
        Span::styled(relay, Style::default().fg(DIM_TEXT).bg(row_bg)),
        Span::styled(" ".repeat(pad2_len), Style::default().bg(row_bg)),
    ]));
}

fn render_group_events(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT_CYAN))
        .style(Style::default().bg(DETAIL_BG))
        .title(Span::styled(
            " Chat ",
            Style::default()
                .fg(ACCENT_CYAN)
                .add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    let pane_width = inner.width as usize;

    let lines = if state.features.group_messages.is_empty() {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No group messages yet",
                Style::default().fg(DIM_TEXT),
            )),
        ]
    } else {
        build_message_lines(&state.features.group_messages, pane_width)
    };

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(DETAIL_BG).fg(BODY_TEXT));
    frame.render_widget(paragraph, area);
}

fn build_message_lines(messages: &[MessageLine], pane_width: usize) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    for msg in messages.iter().take(20) {
        let author_col = author_color(&msg.author);
        let short_auth = short_author(&msg.author);
        let author_label = format!("{short_auth}: ");
        let max_body = pane_width.saturating_sub(author_label.chars().count());
        let body = truncate(&msg.content.replace('\n', " "), max_body);
        out.push(Line::from(vec![
            Span::styled(author_label, Style::default().fg(author_col)),
            Span::styled(body, Style::default().fg(BODY_TEXT)),
        ]));
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
