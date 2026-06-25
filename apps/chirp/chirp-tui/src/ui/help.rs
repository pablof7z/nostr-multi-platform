//! Grouped keymap help overlay.
//!
//! Two-column layout inside a centred overlay.  Scrollable via `state.detail_scroll`.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::AppState;
use crate::ui::colors::{ACCENT_CYAN, BODY_TEXT, DIM_TEXT};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Backwards-compatible shim: called by `layout.rs` which the wiring agent
/// will update to use `render` (3-arg form) before merge.
pub fn render(f: &mut Frame, area: Rect) {
    let dummy = AppState::default();
    render_with_state(f, area, &dummy);
}

/// Full render with scroll support via `state.detail_scroll`.
pub fn render_with_state(f: &mut Frame, area: Rect, state: &AppState) {
    let width = (area.width * 80 / 100)
        .max(50)
        .min(area.width.saturating_sub(2));
    let height = (area.height * 80 / 100)
        .max(16)
        .min(area.height.saturating_sub(2));
    let popup = centered(area, width, height);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Help — press ? or Esc to close ")
        .border_style(Style::default().fg(ACCENT_CYAN));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    let scroll = state.detail_scroll;

    f.render_widget(
        Paragraph::new(left_column())
            .style(Style::default().fg(BODY_TEXT))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        cols[0],
    );
    f.render_widget(
        Paragraph::new(right_column())
            .style(Style::default().fg(BODY_TEXT))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        cols[1],
    );
}

// ---------------------------------------------------------------------------
// Content
// ---------------------------------------------------------------------------

fn left_column() -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    section_header(&mut lines, "Global");
    binding(&mut lines, "h/c/g/w/s", "switch tabs");
    binding(&mut lines, "Tab / BackTab", "next / previous tab");
    binding(&mut lines, "?", "toggle this help");
    binding(&mut lines, "/", "open command palette");
    binding(&mut lines, "Esc", "cancel / close overlay");
    binding(&mut lines, "a", "open account switcher");
    binding(&mut lines, "q / Ctrl+C", "quit");
    lines.push(Line::raw(""));

    section_header(&mut lines, "Navigation");
    binding(&mut lines, "j / k", "move selection down / up");
    binding(&mut lines, "Home / End", "jump to top / bottom");
    binding(&mut lines, "h / l", "focus left / right pane");
    binding(&mut lines, "PageUp / PageDn", "page through feed");
    lines.push(Line::raw(""));

    section_header(&mut lines, "Home tab");
    binding(&mut lines, "Enter", "open selected thread");
    binding(&mut lines, "n", "compose new note");
    binding(&mut lines, "r", "reply to selected note");
    binding(&mut lines, "+", "react to selected note");
    binding(&mut lines, "R", "repost selected note");
    binding(&mut lines, "z", "zap selected note");
    binding(&mut lines, "i", "inspect selected note");
    binding(&mut lines, "f", "follow author");
    binding(&mut lines, "p", "open author profile");
    binding(&mut lines, "F", "unfollow author");
    lines.push(Line::raw(""));

    section_header(&mut lines, "Chats tab");
    binding(&mut lines, "j / k", "select conversation");

    lines
}

fn right_column() -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    section_header(&mut lines, "Groups tab");
    binding(&mut lines, "n", "create NIP-29 or MLS group");
    lines.push(Line::raw(""));

    section_header(&mut lines, "Wallet tab");
    binding(&mut lines, "n", "connect wallet");
    binding(&mut lines, "p", "pay invoice");
    lines.push(Line::raw(""));

    section_header(&mut lines, "Settings tab");
    binding(&mut lines, "n", "add relay / account");
    binding(&mut lines, "Enter", "open outbox detail");
    binding(&mut lines, "r", "retry selected publish");
    binding(&mut lines, "d", "cancel / clear publish");
    binding(&mut lines, "Esc", "close outbox detail");
    lines.push(Line::raw(""));

    section_header(&mut lines, "Compose mode");
    binding(&mut lines, "Enter", "publish note");
    binding(&mut lines, "Shift+Enter", "insert newline");
    binding(&mut lines, "Esc Esc", "discard draft");
    lines.push(Line::raw(""));

    section_header(&mut lines, "Palette");
    binding(&mut lines, "j / k", "move selection");
    binding(&mut lines, "Enter", "run command");
    binding(&mut lines, "Esc", "close palette");

    lines
}

// ---------------------------------------------------------------------------
// Line builders
// ---------------------------------------------------------------------------

fn section_header(lines: &mut Vec<Line<'static>>, title: &'static str) {
    lines.push(Line::from(Span::styled(
        title,
        Style::default()
            .fg(ACCENT_CYAN)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    )));
}

fn binding(lines: &mut Vec<Line<'static>>, key: &'static str, desc: &'static str) {
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {:<18}", key),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(desc, Style::default().fg(DIM_TEXT)),
    ]));
}

// ---------------------------------------------------------------------------
// Layout helper
// ---------------------------------------------------------------------------

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width.saturating_sub(2));
    let h = height.min(area.height.saturating_sub(2));
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(area.height.saturating_sub(h) / 2),
            Constraint::Length(h),
            Constraint::Min(0),
        ])
        .split(area);
    let horiz = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(area.width.saturating_sub(w) / 2),
            Constraint::Length(w),
            Constraint::Min(0),
        ])
        .split(vert[1]);
    horiz[1]
}
