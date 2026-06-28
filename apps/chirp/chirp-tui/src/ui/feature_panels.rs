//! Approach-b feature panel dispatcher.
//!
//! Delegates each non-Home tab to its own rendering module:
//!   chats    → ui::chats
//!   groups   → ui::groups
//!   wallet   → ui::wallet
//!   settings → ui::settings

use ratatui::layout::Rect;
use ratatui::Frame;

use crate::app::AppState;
use crate::features::FeatureTab;
use crate::ui::{chats, groups, settings, wallet};

pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    match state.tab {
        FeatureTab::Chats => chats::render(frame, area, state),
        FeatureTab::Groups => groups::render(frame, area, state),
        FeatureTab::Wallet => wallet::render(frame, area, state),
        FeatureTab::Settings => settings::render(frame, area, state),
        FeatureTab::Home => {}
    }
}
