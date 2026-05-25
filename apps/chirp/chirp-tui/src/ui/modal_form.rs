//! Multi-field modal form overlay (Pattern D).
//!
//! Centered overlay rendered when the caller places the app in ModalForm mode.
//! `modal_title`, `modal_fields`, `modal_cursor`, and `modal_action` are not
//! yet on `AppState`; accessors below return defaults until the wiring agent
//! adds the fields.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::AppState;
use crate::ui::colors::{ACCENT_CYAN, BODY_TEXT, DIM_TEXT, FOOTER_BG};

// ---------------------------------------------------------------------------
// Accessors — replace bodies once wiring agent adds fields
// ---------------------------------------------------------------------------

fn modal_title(_state: &AppState) -> &str {
    ""
}

fn modal_fields(_state: &AppState) -> Vec<(String, String)> {
    Vec::new()
}

fn modal_cursor(_state: &AppState) -> usize {
    0
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Render the modal form overlay centred within `area`.
pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let title = modal_title(state);
    let fields = modal_fields(state);
    let cursor = modal_cursor(state);

    let n = fields.len() as u16;
    // height = fields × 2 rows + 2 (borders) + 2 (spacers) + 2 (buttons row)
    let height = n * 2 + 6;
    let width = (area.width * 60 / 100).max(40);

    let popup = centered(area, width, height);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", title))
        .border_style(Style::default().fg(ACCENT_CYAN))
        .style(Style::default().bg(FOOTER_BG));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    if fields.is_empty() {
        return;
    }

    // Build constraints: 1 spacer top + 2 rows per field + 1 spacer + 1 buttons
    let mut constraints = vec![Constraint::Length(1)];
    for _ in &fields {
        constraints.push(Constraint::Length(1)); // label+value row
        constraints.push(Constraint::Length(1)); // underline row
    }
    constraints.push(Constraint::Length(1)); // spacer
    constraints.push(Constraint::Length(1)); // buttons
    constraints.push(Constraint::Min(0));

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    // Render each field (sections[0] is top spacer)
    for (i, (label, value)) in fields.iter().enumerate() {
        let is_active = i == cursor;
        let field_row = sections[1 + i * 2];
        let underline_row = sections[2 + i * 2];

        // Label width — align all values at column 18
        let label_width = 18usize;
        let label_display = format!("{:<width$}", format!("{}:", label), width = label_width);

        let (value_style, underline_char) = if is_active {
            (
                Style::default().fg(BODY_TEXT).add_modifier(Modifier::BOLD),
                "\u{2500}", // ─
            )
        } else {
            (Style::default().fg(DIM_TEXT), " ")
        };

        let line = Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(label_display, Style::default().fg(DIM_TEXT)),
            Span::styled(value.clone(), value_style),
            if is_active {
                Span::styled(
                    "\u{2588}",
                    Style::default()
                        .fg(ACCENT_CYAN)
                        .add_modifier(Modifier::SLOW_BLINK),
                )
            } else {
                Span::raw("")
            },
        ]);
        f.render_widget(Paragraph::new(line), field_row);

        // Underline for active field
        if is_active {
            let ul_width = value.chars().count() + 1; // +1 for cursor
            let ul_x = field_row.x + 2 + label_width as u16;
            let ul_area = Rect {
                x: ul_x,
                y: underline_row.y,
                width: (ul_width as u16).min(field_row.width.saturating_sub(2 + label_width as u16)),
                height: 1,
            };
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    underline_char.repeat(ul_area.width as usize),
                    Style::default().fg(ACCENT_CYAN),
                ))),
                ul_area,
            );
        }
    }

    // Buttons row
    let btn_row_idx = 1 + fields.len() * 2 + 1;
    if btn_row_idx < sections.len().saturating_sub(1) {
        let btn_area = sections[btn_row_idx];
        let buttons = Line::from(vec![
            Span::raw("               "),
            Span::styled(
                "[ Create ]",
                Style::default()
                    .fg(ACCENT_CYAN)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled("[ Cancel ]", Style::default().fg(DIM_TEXT)),
        ]);
        f.render_widget(Paragraph::new(buttons), btn_area);
    }

    // Footer hint embedded in bottom border via a paragraph below
    let hint_area = Rect {
        y: popup.y + popup.height,
        height: 1,
        ..popup
    };
    if hint_area.y < area.y + area.height {
        let hint = Line::from(vec![
            hint_key("Tab"),
            hint_sep(" next  "),
            hint_key("Shift+Tab"),
            hint_sep(" prev  "),
            hint_key("Enter"),
            hint_sep(" submit"),
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

fn hint_key(s: &'static str) -> Span<'static> {
    Span::styled(s, Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD))
}

fn hint_sep(s: &'static str) -> Span<'static> {
    Span::styled(s, Style::default().fg(DIM_TEXT))
}
