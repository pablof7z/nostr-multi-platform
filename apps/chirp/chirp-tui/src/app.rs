use serde_json::Value;

use crate::bridge::NmpEvent;
use crate::feature_snapshot::FeatureSnapshot;
use crate::features::FeatureTab;
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
    Command,
    Palette { cursor: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppState {
    pub focused: Pane,
    pub mode: Mode,
    pub basic: bool,
    pub show_help: bool,
    pub tab: FeatureTab,
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
    pub features: FeatureSnapshot,
    pub selected: usize,
    pub chat_selected: usize,
    pub group_selected: usize,
    pub settings_account_selected: usize,
    pub detail_cursor: usize,
    pub detail_scroll: u16,
    pub compose: String,
    pub reply_to: Option<String>,
    pub command: String,
    pub status: String,
    pub profile_pubkey: String,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            focused: Pane::Feed,
            mode: Mode::Normal,
            basic: false,
            show_help: false,
            tab: FeatureTab::Home,
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
            features: FeatureSnapshot::default(),
            selected: 0,
            chat_selected: 0,
            group_selected: 0,
            settings_account_selected: 0,
            detail_cursor: 0,
            detail_scroll: 0,
            compose: String::new(),
            reply_to: None,
            command: String::new(),
            status: "starting NMP runtime".to_string(),
            profile_pubkey: String::new(),
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
        self.features = FeatureSnapshot::from_payload(&event.payload);
        // Clamp tab-specific selection indices to avoid out-of-bounds access.
        let conv_len = self.features.dm_conversations.len();
        if conv_len == 0 {
            self.chat_selected = 0;
        } else if self.chat_selected >= conv_len {
            self.chat_selected = conv_len - 1;
        }
        let group_len = self.features.discovered_groups.len();
        if group_len == 0 {
            self.group_selected = 0;
        } else if self.group_selected >= group_len {
            self.group_selected = group_len - 1;
        }
        let account_len = self.features.accounts.len();
        if account_len == 0 {
            self.settings_account_selected = 0;
        } else if self.settings_account_selected >= account_len {
            self.settings_account_selected = account_len - 1;
        }
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
            let previous_selected = self.selected;
            if self.selected >= self.rows.len() {
                self.selected = self.rows.len().saturating_sub(1);
            }
            if self.selected != previous_selected {
                self.detail_cursor = 0;
                self.detail_scroll = 0;
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

    pub fn set_tab(&mut self, tab: FeatureTab) {
        self.tab = tab;
        self.status = format!("tab {}", tab.label());
    }

    pub fn next_tab(&mut self) {
        self.set_tab(self.tab.next());
    }

    pub fn previous_tab(&mut self) {
        self.set_tab(self.tab.previous());
    }

    pub fn select_next(&mut self) {
        if self.selected + 1 < self.rows.len() {
            self.selected += 1;
        }
    }

    pub fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn chat_select_next(&mut self) {
        let len = self.features.dm_conversations.len();
        if len > 0 && self.chat_selected + 1 < len {
            self.chat_selected += 1;
        }
    }

    pub fn chat_select_previous(&mut self) {
        self.chat_selected = self.chat_selected.saturating_sub(1);
    }

    pub fn group_select_next(&mut self) {
        let len = self.features.discovered_groups.len();
        if len > 0 && self.group_selected + 1 < len {
            self.group_selected += 1;
        }
    }

    pub fn group_select_previous(&mut self) {
        self.group_selected = self.group_selected.saturating_sub(1);
    }

    pub fn settings_account_select_next(&mut self) {
        let len = self.features.accounts.len();
        if len > 0 && self.settings_account_selected + 1 < len {
            self.settings_account_selected += 1;
        }
    }

    pub fn settings_account_select_previous(&mut self) {
        self.settings_account_selected = self.settings_account_selected.saturating_sub(1);
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

    #[must_use]
    pub fn selected_row(&self) -> Option<&TimelineRow> {
        self.rows.get(self.selected)
    }

    pub fn open_palette(&mut self) {
        self.mode = Mode::Palette { cursor: 0 };
    }

    pub fn close_palette(&mut self) {
        self.mode = Mode::Normal;
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

    pub fn start_command(&mut self) {
        self.mode = Mode::Command;
        self.command.clear();
        self.status = "command mode: type help for Chirp iOS parity commands".to_string();
    }

    pub fn cancel_command(&mut self) {
        self.mode = Mode::Normal;
        self.command.clear();
        self.status = "command canceled".to_string();
    }

    pub fn push_command_char(&mut self, ch: char) {
        self.command.push(ch);
    }

    pub fn backspace_command(&mut self) {
        self.command.pop();
    }

    pub fn take_command(&mut self) -> Option<String> {
        let input = self.command.trim().to_string();
        if input.is_empty() {
            self.status = "command is empty".to_string();
            return None;
        }
        self.command.clear();
        self.mode = Mode::Normal;
        Some(input)
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

use crate::short_id;

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
