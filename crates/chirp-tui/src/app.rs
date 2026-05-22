use serde_json::Value;

use crate::bridge::NmpEvent;
pub use crate::runtime::AppRuntime;
use crate::snapshot::{ActionResult, ActionStageRow, InterestRow, RelayRow, RuntimeMetrics};
use crate::timeline::TimelineRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Feed,
    Detail,
    Profile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Compose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppState {
    pub focused: Pane,
    pub mode: Mode,
    pub basic: bool,
    pub show_help: bool,
    pub tab: &'static str,
    pub update_count: u64,
    pub blocks: usize,
    pub cards: usize,
    pub rows: Vec<TimelineRow>,
    pub metrics: RuntimeMetrics,
    pub relays: Vec<RelayRow>,
    pub interests: Vec<InterestRow>,
    pub pending_actions: Vec<String>,
    pub action_stages: Vec<ActionStageRow>,
    pub last_action_result: Option<ActionResult>,
    pub selected: usize,
    pub compose: String,
    pub reply_to: Option<String>,
    pub status: String,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            focused: Pane::Feed,
            mode: Mode::Normal,
            basic: false,
            show_help: false,
            tab: "home",
            update_count: 0,
            blocks: 0,
            cards: 0,
            rows: Vec::new(),
            metrics: RuntimeMetrics::default(),
            relays: Vec::new(),
            interests: Vec::new(),
            pending_actions: Vec::new(),
            action_stages: Vec::new(),
            last_action_result: None,
            selected: 0,
            compose: String::new(),
            reply_to: None,
            status: "starting NMP runtime".to_string(),
        }
    }
}

impl AppState {
    pub fn apply_nmp_event(&mut self, runtime: &AppRuntime, event: NmpEvent) {
        self.update_count += 1;
        let shared = crate::snapshot::SharedSnapshot::from_payload(&event.payload);
        self.metrics = shared.metrics;
        self.relays = shared.relays;
        self.interests = shared.interests;
        self.action_stages = shared.action_stages;
        let applied_action_result = self.apply_action_results(runtime, shared.action_results);
        if let Some(snapshot) = runtime.chirp_snapshot() {
            self.blocks = snapshot
                .get("blocks")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            self.cards = snapshot
                .get("cards")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            self.rows = TimelineRow::from_snapshot(&snapshot);
            if self.selected >= self.rows.len() {
                self.selected = self.rows.len().saturating_sub(1);
            }
        }
        if !applied_action_result {
            self.status = format!(
                "received NMP update #{} ({} bytes)",
                self.update_count,
                event.payload.len()
            );
        }
    }

    pub fn focus(&mut self, pane: Pane) {
        self.focused = pane;
    }

    pub fn set_basic(&mut self) {
        self.basic = true;
        self.focused = Pane::Feed;
        self.status = "basic mode: single-pane terminal layout".to_string();
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    pub fn close_help(&mut self) -> bool {
        let was_open = self.show_help;
        self.show_help = false;
        was_open
    }

    pub fn select_next(&mut self) {
        if self.selected + 1 < self.rows.len() {
            self.selected += 1;
        }
    }

    pub fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn select_page_down(&mut self) {
        self.selected = (self.selected + 10).min(self.rows.len().saturating_sub(1));
    }

    pub fn select_page_up(&mut self) {
        self.selected = self.selected.saturating_sub(10);
    }

    pub fn select_first(&mut self) {
        self.selected = 0;
    }

    pub fn select_last(&mut self) {
        self.selected = self.rows.len().saturating_sub(1);
    }

    pub fn selected_row(&self) -> Option<&TimelineRow> {
        self.rows.get(self.selected)
    }

    pub fn start_compose(&mut self) {
        self.mode = Mode::Compose;
        self.compose.clear();
        self.reply_to = None;
        self.status = "compose note: Ctrl+Enter publishes, Esc cancels".to_string();
    }

    pub fn start_reply(&mut self) {
        let Some(row) = self.selected_row() else {
            self.status = "select a note before replying".to_string();
            return;
        };
        let row_id = row.id.clone();
        self.mode = Mode::Compose;
        self.compose.clear();
        self.reply_to = Some(row_id.clone());
        self.status = format!("replying to {}", short_id(&row_id));
    }

    pub fn cancel_compose(&mut self) {
        self.mode = Mode::Normal;
        self.compose.clear();
        self.reply_to = None;
        self.status = "compose canceled".to_string();
    }

    pub fn push_compose_char(&mut self, ch: char) {
        self.compose.push(ch);
    }

    pub fn push_compose_newline(&mut self) {
        self.compose.push('\n');
    }

    pub fn backspace_compose(&mut self) {
        self.compose.pop();
    }

    pub fn take_compose(&mut self) -> Option<(String, Option<String>)> {
        let content = self.compose.trim().to_string();
        if content.is_empty() {
            self.status = "compose is empty".to_string();
            return None;
        }
        let reply_to = self.reply_to.take();
        self.compose.clear();
        self.mode = Mode::Normal;
        Some((content, reply_to))
    }

    pub fn track_action(&mut self, correlation_id: String, label: &str) {
        if !self.pending_actions.contains(&correlation_id) {
            self.pending_actions.push(correlation_id.clone());
        }
        self.status = format!("{label} accepted ({})", short_id(&correlation_id));
    }

    fn apply_action_results(&mut self, runtime: &AppRuntime, results: Vec<ActionResult>) -> bool {
        if results.is_empty() {
            return false;
        }

        for result in results {
            self.pending_actions
                .retain(|id| id != &result.correlation_id);
            let _ = runtime.ack_action_stage(&result.correlation_id);
            let message = match result.error.as_deref() {
                Some(error) if !error.is_empty() => format!(
                    "action {} {}: {}",
                    short_id(&result.correlation_id),
                    result.status,
                    error
                ),
                _ => format!(
                    "action {} {}",
                    short_id(&result.correlation_id),
                    result.status
                ),
            };
            self.status = message;
            self.last_action_result = Some(result);
        }
        true
    }
}

fn short_id(value: &str) -> String {
    if value.len() <= 16 {
        value.to_string()
    } else {
        format!("{}...{}", &value[..8], &value[value.len() - 6..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_changes_active_pane() {
        let mut state = AppState::default();
        state.focus(Pane::Profile);
        assert_eq!(state.focused, Pane::Profile);
    }

    #[test]
    fn compose_can_be_taken_for_publish() {
        let mut state = AppState::default();
        state.start_compose();
        state.push_compose_char('h');
        state.push_compose_char('i');

        let payload = state.take_compose();

        assert_eq!(payload, Some(("hi".to_string(), None)));
        assert_eq!(state.mode, Mode::Normal);
    }
}
