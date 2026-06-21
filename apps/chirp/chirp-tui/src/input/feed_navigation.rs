use crate::app::dynamic_feeds::DynamicFeedRuntime;
use crate::app::{AppState, Pane};
use crate::features::FeatureTab;

use super::outbox;

pub(super) fn handle_escape(state: &mut AppState, runtime: &impl DynamicFeedRuntime) {
    if state.tab == FeatureTab::Settings && outbox::close(state) {
        return;
    }
    if state.close_help() {
        return;
    }
    close_current_dynamic_view(state, runtime);
}

pub(super) fn close_current_dynamic_view(state: &mut AppState, runtime: &impl DynamicFeedRuntime) {
    match state.close_current_dynamic_view(runtime) {
        Ok(closed) => {
            state.status = closed.status_label().to_string();
        }
        Err(error) => {
            state.status = format!("close feed failed: {error}");
        }
    }
}

pub(super) fn close_author_before_detail_focus(
    state: &mut AppState,
    runtime: &impl DynamicFeedRuntime,
) -> bool {
    match state.close_author_feed(runtime) {
        Ok(Some(_)) => {
            state.status = "closed profile feed".to_string();
            true
        }
        Ok(None) => true,
        Err(error) => {
            state.status = format!("close profile failed: {error}");
            false
        }
    }
}

pub(super) fn close_thread_before_profile_focus(
    state: &mut AppState,
    runtime: &impl DynamicFeedRuntime,
) {
    match state.close_thread_feed(runtime) {
        Ok(Some(_)) => {
            state.status = "closed thread feed".to_string();
            state.focus(Pane::Profile);
        }
        Ok(None) => state.focus(Pane::Profile),
        Err(error) => state.status = format!("close thread failed: {error}"),
    }
}

pub(super) fn set_tab_closing_dynamic(
    state: &mut AppState,
    runtime: &impl DynamicFeedRuntime,
    tab: FeatureTab,
) {
    if tab != FeatureTab::Home {
        if let Err(error) = state.close_dynamic_feeds(runtime) {
            state.status = format!("close feed failed: {error}");
            return;
        }
        state.focus(Pane::Feed);
    }
    state.set_tab(tab);
}
