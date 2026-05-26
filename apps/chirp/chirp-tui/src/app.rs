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
    /// Legacy command line (`:foo bar`). The `:` key no longer opens this mode
    /// after the chirp-tui UX redesign — it is kept here so the existing
    /// `commands::execute` pipeline still has a parking spot for power-user
    /// callers that want to drive it directly.
    Command,
    Palette { cursor: usize },
    /// Bottom-bar single-field input (Pattern A — nsec import, relay add, …).
    InputBar,
    /// Multi-field centered overlay (Pattern D — create account, bunker
    /// connect, …).
    ModalForm,
    /// Account list overlay invoked from the `a` key.
    AccountSwitcher,
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

    // Input bar (Pattern A) — single-field bottom-bar input.
    pub input_bar_label: String,
    pub input_bar_value: String,
    pub input_bar_masked: bool,
    /// Tag identifying which dispatch handler runs on Enter, e.g. "nsec",
    /// "nwc", "bolt11", "relay", "zap-amount", "dm-npub".
    pub input_bar_action: String,

    // Modal form (Pattern D) — multi-field centered overlay.
    pub modal_title: String,
    pub modal_fields: Vec<(String, String)>,
    pub modal_cursor: usize,
    /// Tag identifying which dispatch handler runs on Enter, e.g.
    /// "create-account", "bunker-connect".
    pub modal_action: String,

    // Account switcher overlay.
    pub account_switcher_cursor: usize,

    /// Toast queue: each entry is `(message, ttl_ticks)`. TTL counts down from
    /// 50 (~5s at 10 Hz). Expired toasts are dropped by `tick_toasts`.
    pub toasts: Vec<(String, u8)>,

    // Inline compose for chats/groups tabs.
    pub chat_composing: bool,
    pub chat_compose_buf: String,
    pub group_composing: bool,
    pub group_compose_buf: String,

    /// Settings section cursor (0=Account, 1=Relays, 2=Outbox, 3=Keys,
    /// 4=Appearance, 5=About).
    pub settings_cursor: usize,

    /// Set by `:zap` while waiting for `nmp.nip57.zap` to surface the
    /// bolt11 via `ShowToast`. When the next snapshot carries a
    /// `last_error_toast` starting with `"Zap invoice: "`, the host
    /// extracts the bolt11 and auto-pays through NWC, then clears this
    /// flag. Mirrors the iOS/Swift host pattern documented in
    /// `nmp-nip57/src/lnurl/mod.rs` (V-43 future fix: kernel auto-pay).
    pub pending_zap_pay: bool,
    /// Recipient pubkey carried across the palette → input-bar → dispatch
    /// boundary for the `"zap-amount"` input bar action.
    pub pending_zap_pubkey: Option<String>,
    /// Event id for the same boundary — produces the `e` tag on a targeted
    /// zap. `None` for a profile-only zap.
    pub pending_zap_event_id: Option<String>,
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
            input_bar_label: String::new(),
            input_bar_value: String::new(),
            input_bar_masked: false,
            input_bar_action: String::new(),
            modal_title: String::new(),
            modal_fields: Vec::new(),
            modal_cursor: 0,
            modal_action: String::new(),
            account_switcher_cursor: 0,
            toasts: Vec::new(),
            chat_composing: false,
            chat_compose_buf: String::new(),
            group_composing: false,
            group_compose_buf: String::new(),
            settings_cursor: 0,
            pending_zap_pay: false,
            pending_zap_pubkey: None,
            pending_zap_event_id: None,
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

        // `nmp.nip57.zap` is async-completing: the LNURL fetcher surfaces
        // the bolt11 via `ShowToast { "Zap invoice: <bolt11>" }`, which
        // lands in the kernel snapshot as `last_error_toast`. While a
        // zap is pending, watch for that toast and auto-pay over NWC.
        // (V-43 will move auto-pay into the kernel; until then the host
        // bridges the two-leg flow.)
        if self.pending_zap_pay {
            if let Some(toast) = parse_last_error_toast(&event.payload) {
                if let Some(bolt11) = toast.strip_prefix("Zap invoice: ") {
                    let bolt11 = bolt11.trim().to_string();
                    match runtime.wallet_pay_invoice(&bolt11, None) {
                        Ok(()) => {
                            let preview = &bolt11[..bolt11.len().min(20)];
                            self.status = format!("Zap payment sent: {preview}\u{2026}");
                        }
                        Err(e) => {
                            self.status = format!("Zap pay failed: {e}");
                        }
                    }
                    self.pending_zap_pay = false;
                } else if toast.starts_with("Zap failed:") {
                    self.status = toast;
                    self.pending_zap_pay = false;
                }
            }
        }

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
        if matches!(self.mode, Mode::Palette { .. }) {
            self.mode = Mode::Normal;
        }
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

    // -----------------------------------------------------------------
    // Input bar (Pattern A)
    // -----------------------------------------------------------------

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

    // -----------------------------------------------------------------
    // Modal form (Pattern D)
    // -----------------------------------------------------------------

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

    // -----------------------------------------------------------------
    // Account switcher
    // -----------------------------------------------------------------

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

    // -----------------------------------------------------------------
    // Toast queue
    // -----------------------------------------------------------------

    pub fn push_toast(&mut self, msg: &str) {
        self.toasts.push((msg.to_string(), 50));
    }

    pub fn tick_toasts(&mut self) {
        for entry in &mut self.toasts {
            entry.1 = entry.1.saturating_sub(1);
        }
        self.toasts.retain(|(_, ttl)| *ttl > 0);
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

/// Pull `last_error_toast` out of a snapshot payload. Snapshots arrive
/// wrapped as `{"t":"snapshot","v":<snapshot>}` — the toast lives inside
/// `v`. Falls back to the bare snapshot shape for parity with test
/// fixtures (mirrors `SharedSnapshot::from_payload`).
fn parse_last_error_toast(payload: &str) -> Option<String> {
    let value: Value = serde_json::from_str(payload).ok()?;
    let root = value.get("v").unwrap_or(&value);
    root.get("last_error_toast")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
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

    // `parse_last_error_toast` is the seam the zap auto-pay watches; pin
    // every shape it must handle so a future snapshot envelope change
    // fails loudly.

    #[test]
    fn parse_last_error_toast_unwraps_snapshot_envelope() {
        let payload = serde_json::json!({
            "t": "snapshot",
            "v": { "last_error_toast": "Zap invoice: lnbcDEAD" }
        })
        .to_string();
        assert_eq!(
            parse_last_error_toast(&payload),
            Some("Zap invoice: lnbcDEAD".to_string())
        );
    }

    #[test]
    fn parse_last_error_toast_reads_bare_snapshot() {
        let payload = serde_json::json!({
            "last_error_toast": "Zap failed: bunker signing not yet supported"
        })
        .to_string();
        assert_eq!(
            parse_last_error_toast(&payload),
            Some("Zap failed: bunker signing not yet supported".to_string())
        );
    }

    #[test]
    fn parse_last_error_toast_returns_none_when_missing() {
        let payload = serde_json::json!({ "t": "snapshot", "v": {} }).to_string();
        assert_eq!(parse_last_error_toast(&payload), None);
    }

    #[test]
    fn parse_last_error_toast_returns_none_for_empty_string() {
        let payload = serde_json::json!({
            "t": "snapshot",
            "v": { "last_error_toast": "" }
        })
        .to_string();
        assert_eq!(parse_last_error_toast(&payload), None);
    }

    #[test]
    fn parse_last_error_toast_returns_none_for_invalid_json() {
        assert_eq!(parse_last_error_toast("not json"), None);
    }
}
