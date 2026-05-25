//! Approach-b Home tab orchestrator.
//!
//! Layout:
//!   - Left column (38%): post_list (75%) above relay_panel (25%)
//!   - Right column (62%): post_detail

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};

use crate::app::AppState;
use crate::ui::{post_detail, post_list, relay_panel};

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let cols =
        Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)]).split(area);

    let left =
        Layout::vertical([Constraint::Percentage(75), Constraint::Percentage(25)]).split(cols[0]);

    post_list::render(f, left[0], state);
    relay_panel::render(f, left[1], state);
    post_detail::render(f, cols[1], state);
}
