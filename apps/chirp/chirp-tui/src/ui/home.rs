//! Approach-b Home tab orchestrator.
//!
//! Layout:
//!   - Left column (38%): post_list or profile_pane (75%) above relay_panel (25%)
//!   - Right column (62%): post_detail

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::Frame;

use crate::app::{AppState, Mode, Pane};
use crate::ui::layout::RenderContext;
use crate::ui::{palette, post_detail, post_list, profile_pane, relay_panel};

pub fn render(f: &mut Frame, area: Rect, state: &AppState, context: &RenderContext<'_>) {
    let cols =
        Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)]).split(area);

    let left =
        Layout::vertical([Constraint::Percentage(75), Constraint::Percentage(25)]).split(cols[0]);

    if state.focused == Pane::Profile {
        profile_pane::render(f, left[0], state);
    } else {
        post_list::render(f, left[0], state);
    }
    relay_panel::render(f, left[1], state);
    post_detail::render(f, cols[1], state, context);

    if let Mode::Palette { cursor } = state.mode {
        palette::render(f, area, state, cursor);
    }
}
