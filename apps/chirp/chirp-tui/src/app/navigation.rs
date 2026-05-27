use crate::app::{AppRuntime, AppState, OutboxSelection};
use crate::timeline::TimelineRow;

impl AppState {
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

    pub fn settings_relay_select_next(&mut self) {
        if !self.relays.is_empty() && self.settings_relay_selected + 1 < self.relays.len() {
            self.settings_relay_selected += 1;
        }
    }

    pub fn settings_relay_select_previous(&mut self) {
        self.settings_relay_selected = self.settings_relay_selected.saturating_sub(1);
    }

    pub fn settings_section_next(&mut self) {
        self.settings_cursor = (self.settings_cursor + 1).min(2);
        if self.settings_cursor != 2 {
            self.outbox_selected = None;
        }
    }

    pub fn settings_section_previous(&mut self) {
        self.settings_cursor = self.settings_cursor.saturating_sub(1);
        if self.settings_cursor != 2 {
            self.outbox_selected = None;
        }
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

    pub fn load_older_timeline_if_needed(&mut self, runtime: &AppRuntime) {
        if self.timeline_has_more && self.selected.saturating_add(5) >= self.rows.len() {
            self.load_older_timeline(runtime);
        }
    }

    pub fn load_older_timeline(&mut self, runtime: &AppRuntime) {
        if !self.timeline_has_more {
            return;
        }
        runtime.chirp_load_older_timeline();
    }

    #[must_use]
    pub fn selected_row(&self) -> Option<&TimelineRow> {
        self.rows.get(self.selected)
    }

    pub fn clamp_outbox_selection(&mut self) {
        let active_len = self.features.outbox.len();
        let history_len = self.features.history.len();
        self.outbox_selected = match self.outbox_selected {
            Some(OutboxSelection::Active(_)) if active_len == 0 && history_len > 0 => {
                Some(OutboxSelection::History(0))
            }
            Some(OutboxSelection::History(_)) if history_len == 0 && active_len > 0 => {
                Some(OutboxSelection::Active(0))
            }
            Some(_) if active_len == 0 && history_len == 0 => None,
            Some(OutboxSelection::Active(i)) if i >= active_len => {
                Some(OutboxSelection::Active(active_len.saturating_sub(1)))
            }
            Some(OutboxSelection::History(i)) if i >= history_len => {
                Some(OutboxSelection::History(history_len.saturating_sub(1)))
            }
            other => other,
        };
    }
}
