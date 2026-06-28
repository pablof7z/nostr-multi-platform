use crate::app::{AppState, Mode, ReplyTarget};
use crate::short_id;

impl AppState {
    pub fn start_compose(&mut self) {
        self.mode = Mode::Compose;
        self.compose.clear();
        self.reply_to = None;
        self.status = "compose note: Enter sends, Shift+Enter for newline, Esc cancels".to_string();
    }

    pub fn start_reply(&mut self) {
        let Some(row) = self.selected_row() else {
            self.status = "select a note before replying".to_string();
            return;
        };
        let target = ReplyTarget {
            id: row.id.clone(),
            author_pubkey: row.author_pubkey.clone(),
            content: row.content.clone(),
            created_at: row.created_at,
        };
        let row_id = target.id.clone();
        self.mode = Mode::Compose;
        self.compose.clear();
        self.reply_to = Some(target);
        self.status = format!(
            "replying to {}: Enter sends, Shift+Enter for newline, Esc cancels",
            short_id(&row_id)
        );
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

    pub fn take_compose(&mut self) -> Option<(String, Option<ReplyTarget>)> {
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

    pub fn start_input_bar(&mut self, label: &str, masked: bool, action: &str) {
        self.mode = Mode::InputBar;
        self.input_bar_label = label.to_string();
        self.input_bar_value.clear();
        self.input_bar_masked = masked;
        self.input_bar_action = action.to_string();
    }

    pub fn push_input_char(&mut self, ch: char) {
        self.input_bar_value.push(ch);
    }

    pub fn backspace_input(&mut self) {
        self.input_bar_value.pop();
    }

    pub fn take_input(&mut self) -> Option<(String, String)> {
        let value = self.input_bar_value.trim().to_string();
        if value.is_empty() {
            self.push_toast("input is empty");
            return None;
        }
        let action = std::mem::take(&mut self.input_bar_action);
        self.input_bar_value.clear();
        self.input_bar_label.clear();
        self.input_bar_masked = false;
        self.mode = Mode::Normal;
        Some((action, value))
    }

    pub fn cancel_input(&mut self) {
        self.mode = Mode::Normal;
        self.input_bar_value.clear();
        self.input_bar_label.clear();
        self.input_bar_action.clear();
        self.input_bar_masked = false;
        self.push_toast("canceled");
    }

    pub fn start_modal(&mut self, title: &str, fields: Vec<&str>, action: &str) {
        self.mode = Mode::ModalForm;
        self.modal_title = title.to_string();
        self.modal_fields = fields
            .into_iter()
            .map(|label| (label.to_string(), String::new()))
            .collect();
        self.modal_cursor = 0;
        self.modal_action = action.to_string();
    }

    pub fn push_modal_char(&mut self, ch: char) {
        if let Some((_, value)) = self.modal_fields.get_mut(self.modal_cursor) {
            value.push(ch);
        }
    }

    pub fn backspace_modal(&mut self) {
        if let Some((_, value)) = self.modal_fields.get_mut(self.modal_cursor) {
            value.pop();
        }
    }

    pub fn next_modal_field(&mut self) {
        let n = self.modal_fields.len();
        if n == 0 {
            return;
        }
        self.modal_cursor = (self.modal_cursor + 1) % n;
    }

    pub fn prev_modal_field(&mut self) {
        let n = self.modal_fields.len();
        if n == 0 {
            return;
        }
        self.modal_cursor = (self.modal_cursor + n - 1) % n;
    }

    pub fn take_modal(&mut self) -> Option<(String, Vec<(String, String)>)> {
        if self.modal_fields.is_empty() {
            self.push_toast("modal has no fields");
            return None;
        }
        let action = std::mem::take(&mut self.modal_action);
        let fields = std::mem::take(&mut self.modal_fields);
        self.modal_title.clear();
        self.modal_cursor = 0;
        self.mode = Mode::Normal;
        Some((action, fields))
    }

    pub fn cancel_modal(&mut self) {
        self.mode = Mode::Normal;
        self.modal_title.clear();
        self.modal_fields.clear();
        self.modal_cursor = 0;
        self.modal_action.clear();
        self.push_toast("canceled");
    }

    pub fn open_note_details_modal(&mut self, content: String) {
        self.note_details_content = content;
        self.mode = Mode::NoteDetailsModal { scroll: 0 };
    }

    pub fn close_note_details_modal(&mut self) {
        self.mode = Mode::Normal;
        self.note_details_content.clear();
    }

    pub fn scroll_note_details_modal_down(&mut self) {
        if let Mode::NoteDetailsModal { ref mut scroll } = self.mode {
            *scroll = scroll.saturating_add(1);
        }
    }

    pub fn scroll_note_details_modal_up(&mut self) {
        if let Mode::NoteDetailsModal { ref mut scroll } = self.mode {
            *scroll = scroll.saturating_sub(1);
        }
    }

    pub fn open_account_switcher(&mut self) {
        self.mode = Mode::AccountSwitcher;
        if let Some(idx) = self.features.accounts.iter().position(|a| a.active) {
            self.account_switcher_cursor = idx;
        } else {
            self.account_switcher_cursor = 0;
        }
    }

    pub fn close_account_switcher(&mut self) {
        self.mode = Mode::Normal;
    }
}
