//! Settings tab: accounts, full relay inventory, relay detail, and outbox.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::AppState;
use crate::feature_snapshot::AccountLine;
use crate::ui::colors::{
    author_color, ACCENT_CYAN, BODY_TEXT, DIMMER_TEXT, DIM_TEXT, LIST_BG, SELECTED_BG, ZAP,
};
use crate::ui::{outbox, relay_settings};

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(22),
            Constraint::Percentage(30),
            Constraint::Percentage(33),
            Constraint::Percentage(15),
        ])
        .split(area);

    render_accounts(frame, cols[0], state, state.settings_cursor == 0);
    relay_settings::render_relay_list(frame, cols[1], state, state.settings_cursor == 1);
    relay_settings::render_relay_detail(frame, cols[2], state);
    outbox::render_outbox(frame, cols[3], state);
}

fn render_accounts(frame: &mut Frame, area: Rect, state: &AppState, active: bool) {
    let border = if active { ACCENT_CYAN } else { DIMMER_TEXT };
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(border))
        .style(Style::default().bg(LIST_BG))
        .title(Span::styled(
            " Accounts ",
            Style::default()
                .fg(ACCENT_CYAN)
                .add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    let pane_width = inner.width as usize;

    let lines = if state.features.accounts.is_empty() {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No accounts configured",
                Style::default().fg(DIM_TEXT),
            )),
        ]
    } else {
        let mut all: Vec<Line<'static>> = Vec::new();
        for (i, account) in state.features.accounts.iter().enumerate() {
            let selected = i == state.settings_account_selected;
            append_account_card(&mut all, account, selected, pane_width);
        }
        all
    };

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(LIST_BG).fg(BODY_TEXT));
    frame.render_widget(paragraph, area);
}

fn append_account_card(
    lines: &mut Vec<Line<'static>>,
    account: &AccountLine,
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

    // Row 1: active marker + display name
    let active_marker = if account.active {
        Span::styled("* ", Style::default().fg(ZAP).bg(row_bg))
    } else {
        Span::styled("  ", Style::default().bg(row_bg))
    };
    let name_max = content_width.saturating_sub(2);
    let name = truncate(&account.display, name_max);
    let name_len = name.chars().count();
    let col = author_color(&account.npub);
    let name_span = Span::styled(
        name,
        Style::default()
            .fg(col)
            .bg(row_bg)
            .add_modifier(Modifier::BOLD),
    );
    let pad1_len = content_width.saturating_sub(2 + name_len);
    lines.push(Line::from(vec![
        gutter.clone(),
        active_marker,
        name_span,
        Span::styled(" ".repeat(pad1_len), Style::default().bg(row_bg)),
    ]));

    // Row 2: signer type badge
    let signer = truncate(&account.signer, content_width);
    let signer_len = signer.chars().count();
    let pad2_len = content_width.saturating_sub(signer_len);
    lines.push(Line::from(vec![
        gutter,
        Span::styled(signer, Style::default().fg(DIMMER_TEXT).bg(row_bg)),
        Span::styled(" ".repeat(pad2_len), Style::default().bg(row_bg)),
    ]));
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
