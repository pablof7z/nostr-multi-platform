//! Bottom-bar input widget (Pattern A).
//!
//! Two-row overlay rendered when the caller places the app in InputBar mode.
//! Today `input_bar_label`, `input_bar_value`, and `input_bar_masked` do not
//! exist on `AppState`; the accessor helpers below return empty/false until
//! the wiring agent adds the fields.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::AppState;
use crate::ui::colors::{ACCENT_CYAN, BODY_TEXT, DIM_TEXT, FOOTER_BG};

// ---------------------------------------------------------------------------
// Accessors — swap to real fields once wiring agent lands
// ---------------------------------------------------------------------------

fn input_label(_state: &AppState) -> &str {
    ""
}

fn input_value(_state: &AppState) -> &str {
    ""
}

fn input_masked(_state: &AppState) -> bool {
    false
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Render the input bar.
///
/// `area` must be exactly 2 rows tall (caller's responsibility).  If the
/// area is too small we render nothing.
pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    if area.height < 2 {
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(area);

    render_input_row(f, rows[0], state);
    render_hint_row(f, rows[1]);
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn render_input_row(f: &mut Frame, area: Rect, state: &AppState) {
    let label = input_label(state);
    let value = input_value(state);
    let masked = input_masked(state);

    // Build displayed value: bullets when masked, real chars otherwise.
    let display_value: String = if masked {
        "\u{25cf}".repeat(value.chars().count())
    } else {
        value.to_string()
    };

    // Separator between label and value.
    let sep = if label.is_empty() { "" } else { " \u{203a}  " };

    // Block with top border only via a synthetic top-border paragraph.
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT_CYAN))
        .style(Style::default().bg(FOOTER_BG));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let line = Line::from(vec![
        Span::styled(
            format!("{}{}", label, sep),
            Style::default().fg(DIM_TEXT),
        ),
        Span::styled(
            display_value,
            Style::default().fg(BODY_TEXT),
        ),
        // Fake cursor block
        Span::styled(
            "\u{2588}",
            Style::default()
                .fg(ACCENT_CYAN)
                .add_modifier(Modifier::SLOW_BLINK),
        ),
    ]);
    f.render_widget(Paragraph::new(line), inner);
}

fn render_hint_row(f: &mut Frame, area: Rect) {
    let hints = Line::from(vec![
        hint_key("Enter"),
        hint_sep(" confirm  "),
        hint_key("Esc"),
        hint_sep(" cancel  "),
        hint_key("Ctrl+V"),
        hint_sep(" paste"),
    ]);
    f.render_widget(
        Paragraph::new(hints).style(Style::default().bg(FOOTER_BG)),
        area,
    );
}

fn hint_key(s: &'static str) -> Span<'static> {
    Span::styled(s, Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD))
}

fn hint_sep(s: &'static str) -> Span<'static> {
    Span::styled(s, Style::default().fg(DIM_TEXT))
}
