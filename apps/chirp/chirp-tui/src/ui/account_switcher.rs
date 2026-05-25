//! Account-switcher overlay.
//!
//! Centered overlay listing all accounts from `state.features.accounts`.
//! `account_switcher_cursor` is not yet on `AppState`; the accessor below
//! returns 0 until the wiring agent adds the field.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use ratatui::Frame;

use crate::app::AppState;
use crate::ui::colors::{ACCENT_CYAN, DIM_TEXT, RELAY_OK, SELECTED_BG};

// ---------------------------------------------------------------------------
// Accessor — replace with `state.account_switcher_cursor` post-wiring
// ---------------------------------------------------------------------------

fn switcher_cursor(state: &AppState) -> usize {
    state.account_switcher_cursor
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Render the account-switcher overlay centred within `area`.
pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let accounts = &state.features.accounts;
    let cursor = switcher_cursor(state);

    // height: 1 top spacer + accounts × 1 row + 1 spacer + 1 add-account + 1 spacer + 2 borders
    let n = accounts.len().max(1) as u16;
    let height = n + 6;
    let width = 44u16.min(area.width.saturating_sub(4));

    let popup = centered(area, width, height);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Switch account ")
        .border_style(Style::default().fg(ACCENT_CYAN));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    // Split inner: list area + spacer + [n] add account + hint footer
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // account list
            Constraint::Length(1), // spacer
            Constraint::Length(1), // add account
            Constraint::Length(1), // spacer
        ])
        .split(inner);

    // Build account list items
    let items: Vec<ListItem> = accounts
        .iter()
        .enumerate()
        .map(|(i, acc)| {
            let is_active = acc.active;
            let is_selected = i == cursor;

            let bullet = if is_active { "\u{25cf}" } else { "\u{25cb}" };
            let bullet_color = if is_active { RELAY_OK } else { DIM_TEXT };

            let npub_short = if acc.npub.len() > 12 {
                format!("{}…{}", &acc.npub[..8], &acc.npub[acc.npub.len() - 4..])
            } else {
                acc.npub.clone()
            };

            let bg = if is_selected {
                Style::default().bg(SELECTED_BG)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("  {} ", bullet),
                    Style::default().fg(bullet_color).patch(bg),
                ),
                Span::styled(
                    format!("{:<10}", truncate(&acc.display, 10)),
                    Style::default()
                        .fg(if is_active {
                            RELAY_OK
                        } else {
                            ratatui::style::Color::Reset
                        })
                        .add_modifier(if is_active { Modifier::BOLD } else { Modifier::empty() })
                        .patch(bg),
                ),
                Span::styled(
                    format!("  {}  ", npub_short),
                    Style::default().fg(DIM_TEXT).patch(bg),
                ),
                Span::styled(
                    truncate(&acc.signer, 8),
                    Style::default().fg(DIM_TEXT).patch(bg),
                ),
            ]))
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, sections[0]);

    // Add account shortcut
    let add = Line::from(vec![
        Span::styled("  [n] ", Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD)),
        Span::styled("Add account", Style::default().fg(DIM_TEXT)),
    ]);
    f.render_widget(Paragraph::new(add), sections[2]);

    // Hint below box
    let hint_y = popup.y + popup.height;
    if hint_y < area.y + area.height {
        let hint_area = Rect {
            y: hint_y,
            height: 1,
            ..popup
        };
        let hint = Line::from(vec![
            hint_key("j/k"),
            hint_sep(" move  "),
            hint_key("Enter"),
            hint_sep(" switch  "),
            hint_key("Esc"),
        ]);
        f.render_widget(Paragraph::new(hint), hint_area);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width.saturating_sub(4));
    let h = height.min(area.height.saturating_sub(4));
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

fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() > max {
        chars[..max].iter().collect()
    } else {
        s.to_string()
    }
}

fn hint_key(s: &'static str) -> Span<'static> {
    Span::styled(s, Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD))
}

fn hint_sep(s: &'static str) -> Span<'static> {
    Span::styled(s, Style::default().fg(DIM_TEXT))
}
