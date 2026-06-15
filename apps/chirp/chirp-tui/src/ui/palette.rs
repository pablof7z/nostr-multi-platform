//! Command palette modal — context-aware action overlay.
//!
//! Opens with `/` in Normal mode, closes with Esc or Enter.
//! Action list adapts based on whether a reply is focused in the Detail pane.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{AppState, Pane};
use crate::ui::colors::{ACCENT_CYAN, BODY_TEXT, DETAIL_BG, DIM_TEXT, SELECTED_BG};

const REPLY_ACTIONS: &[&str] = &["View profile", "Follow", "Unfollow", "Reply"];
const ALL_ACTIONS: &[&str] = &[
    "View profile",
    "React \u{2665}",
    "Follow",
    "Unfollow",
    "Repost",
    "Reply",
    "Zap",
    "View note details",
];

/// Return the context-appropriate action list for the current app state.
pub fn actions_for_state(state: &AppState) -> Vec<&'static str> {
    if state.focused == Pane::Detail && state.detail_cursor > 0 {
        REPLY_ACTIONS.to_vec()
    } else {
        ALL_ACTIONS.to_vec()
    }
}

/// Render the command palette as a centered modal overlay.
pub fn render(f: &mut Frame, area: Rect, state: &AppState, cursor: usize) {
    let actions = actions_for_state(state);
    let height = (actions.len() as u16 + 4).min(16);
    let modal = centered_rect(60, height, area);

    // Clear the area behind the modal so previous content doesn't show through.
    f.render_widget(Clear, modal);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Actions ")
        .border_style(Style::default().fg(ACCENT_CYAN))
        .style(Style::default().bg(DETAIL_BG));

    let inner = block.inner(modal);

    let lines: Vec<Line<'static>> = actions
        .iter()
        .enumerate()
        .map(|(i, &action)| {
            let selected = i == cursor;
            let bg = if selected { SELECTED_BG } else { DETAIL_BG };
            let prefix = if selected { "> " } else { "  " };
            let mut style = Style::default().fg(BODY_TEXT).bg(bg);
            if selected {
                style = style.add_modifier(Modifier::BOLD);
            }
            Line::from(vec![Span::styled(format!("{}{}", prefix, action), style)])
        })
        .collect();

    f.render_widget(block, modal);
    let paragraph = Paragraph::new(lines).style(Style::default().bg(DETAIL_BG).fg(DIM_TEXT));
    f.render_widget(paragraph, inner);
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let w = area.width * percent_x / 100;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let h = height.min(area.height);
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}
