use serde_json::Value;

use crate::bridge::NmpEvent;
pub use crate::runtime::AppRuntime;
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
    pub tab: &'static str,
    pub update_count: u64,
    pub blocks: usize,
    pub cards: usize,
    pub rows: Vec<TimelineRow>,
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
            tab: "home",
            update_count: 0,
            blocks: 0,
            cards: 0,
            rows: Vec::new(),
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
        self.status = format!(
            "received NMP update #{} ({} bytes)",
            self.update_count,
            event.payload.len()
        );
    }

    pub fn focus(&mut self, pane: Pane) {
        self.focused = pane;
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
