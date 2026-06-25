use serde_json::Value;

use crate::bridge::NmpEvent;
use crate::feature_snapshot::FeatureSnapshot;
use crate::features::FeatureTab;
pub use crate::runtime::AppRuntime;
use crate::snapshot::{ActionResult, ActionStageRow, InterestRow, RelayRow, RuntimeMetrics};
use crate::timeline::TimelineRow;
use crate::ui::nostr_user::profile_wire::ProfileWire;

pub(crate) mod dynamic_feeds;
mod forms;
mod navigation;

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
    Palette {
        cursor: usize,
    },
    /// Bottom-bar single-field input (Pattern A — nsec import, relay add, …).
    InputBar,
    /// Multi-field centered overlay (Pattern D — create account, bunker
    /// connect, …).
    ModalForm,
    /// Account list overlay invoked from the `a` key.
    AccountSwitcher,
    /// Read-only overlay showing typed diagnostics for the selected note row.
    NoteDetailsModal {
        scroll: u16,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboxSelection {
    Active(usize),
    History(usize),
}

/// The note being replied to, captured from the selected timeline row when
/// the user starts a reply.
///
/// Carries the parent's `author` so the NIP-10 reply builder can emit the
/// `p` re-notification tag (the whole point of post-#916 caller-side reply
/// construction). The home-feed projection does not surface the parent's own
/// `Nip10Refs`, so deep-thread root-forwarding is best-effort: the builder
/// treats this parent as the thread root, which is correct for top-level
/// replies and a reasonable fallback otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyTarget {
    pub id: String,
    pub author_pubkey: String,
    pub content: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppState {
    pub focused: Pane,
    pub mode: Mode,
    pub basic: bool,
    pub show_help: bool,
    pub tab: FeatureTab,
    pub update_count: u64,
    /// Count of root cards in the latest home-feed snapshot (V-80: the feed is
    /// thread-roots-only, so this is also the row count).
    pub cards: usize,
    pub rows: Vec<TimelineRow>,
    pub profile_rows: Vec<TimelineRow>,
    pub thread_rows: Vec<TimelineRow>,
    pub thread_event_id: String,
    pub timeline_has_more: bool,
    pub metrics: RuntimeMetrics,
    pub relays: Vec<RelayRow>,
    pub interests: Vec<InterestRow>,
    pub pending_actions: Vec<String>,
    pub action_stages: Vec<ActionStageRow>,
    pub last_action_result: Option<ActionResult>,
    pub features: FeatureSnapshot,
    /// ADR-0063 (#1671 Lane F) — host-side mirror of the kernel's `refs.profile`
    /// row-delta projection. Stateful across frames (row-deltas merge into it);
    /// the ONLY app-side store of hydrated profile facts (D4). Read via
    /// [`AppState::profile`]; never a second native profile dict.
    pub ref_profiles: nmp_core::refs::RefProfileStore,
    pub selected: usize,
    pub chat_selected: usize,
    pub group_selected: usize,
    pub settings_account_selected: usize,
    pub settings_relay_selected: usize,
    pub detail_cursor: usize,
    pub detail_scroll: u16,
    pub compose: String,
    pub reply_to: Option<ReplyTarget>,
    pub command: String,
    pub status: String,
    pub profile_pubkey: String,

    // Input bar (Pattern A) — single-field bottom-bar input.
    pub input_bar_label: String,
    pub input_bar_value: String,
    pub input_bar_masked: bool,
    /// Tag identifying which dispatch handler runs on Enter, e.g. "nsec",
    /// "nwc", "bolt11", "relay", "zap-amount".
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

    /// Settings section cursor (0=Accounts, 1=Relays, 2=Outbox).
    pub settings_cursor: usize,

    /// Settings → Outbox: when `Some`, the outbox detail pane is open and
    /// focused on either an in-flight item or a settled history item. j/k
    /// navigates both groups; Esc clears.
    /// `None` (the default) means the outbox pane shows a flat list and the
    /// Settings tab's j/k continues to navigate the accounts column.
    pub outbox_selected: Option<OutboxSelection>,

    /// Typed note diagnostics shown in the `NoteDetailsModal` overlay.
    pub note_details_content: String,

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
            cards: 0,
            rows: Vec::new(),
            profile_rows: Vec::new(),
            thread_rows: Vec::new(),
            thread_event_id: String::new(),
            timeline_has_more: false,
            metrics: RuntimeMetrics::default(),
            relays: Vec::new(),
            interests: Vec::new(),
            pending_actions: Vec::new(),
            action_stages: Vec::new(),
            last_action_result: None,
            features: FeatureSnapshot::default(),
            ref_profiles: nmp_core::refs::RefProfileStore::new(),
            selected: 0,
            chat_selected: 0,
            group_selected: 0,
            settings_account_selected: 0,
            settings_relay_selected: 0,
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
            settings_cursor: 0,
            outbox_selected: None,
            pending_zap_pubkey: None,
            pending_zap_event_id: None,
            note_details_content: String::new(),
        }
    }
}

impl AppState {
    pub fn apply_nmp_event(&mut self, runtime: &AppRuntime, event: NmpEvent) {
        self.update_count += 1;
        let shared = crate::snapshot::SharedSnapshot::from_transport_payload(&event.payload);
        self.metrics = shared.metrics;
        self.relays = shared.relays;
        self.interests = shared.interests;
        self.action_stages = shared.action_stages;
        self.features = FeatureSnapshot::from_transport_payload(&event.payload);
        // ADR-0063 (#1671 Lane F) — merge this frame's `refs.profile` row-delta
        // batch into the persistent RefRowCache mirror. The sidecar carries only
        // changed/cleared rows (or a baseline on identity change), so it MUST be
        // applied incrementally against the held store, not decoded per-frame.
        self.apply_refs_profile(&event.payload);
        let relay_len = self.relays.len();
        if relay_len == 0 {
            self.settings_relay_selected = 0;
        } else if self.settings_relay_selected >= relay_len {
            self.settings_relay_selected = relay_len - 1;
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
        self.clamp_outbox_selection();
        let applied_action_result = self.apply_action_results(runtime, shared.action_results);
        if let Some(feed) = shared.home_feed {
            self.apply_feed_snapshot(feed);
        }
        self.apply_dynamic_feeds(&shared.feeds);
        if !applied_action_result {
            self.status = format!(
                "received NMP update #{} ({} bytes)",
                self.update_count,
                event.payload.len()
            );
        }
    }

    /// ADR-0063 (#1671 Lane F) — decode the frame's `refs.profile` sidecar (an
    /// NRRD row-delta batch) and merge it into the persistent [`RefProfileStore`]
    /// under the frame's `(session_id, snapshot_epoch)` identity. A FlatBuffers
    /// frame that lacks the sidecar (or whose envelope fails to decode) is a
    /// no-op — the prior cache is retained (fail-closed, D6). The JSON-fixture
    /// path carries no typed sidecar, so it is skipped.
    fn apply_refs_profile(&mut self, payload: &crate::bridge::UpdatePayload) {
        let crate::bridge::UpdatePayload::FlatBuffers(bytes) = payload else {
            return;
        };
        let Ok(envelope) = nmp_core::decode_snapshot_envelope(bytes) else {
            return;
        };
        let Ok(typed) = nmp_core::decode_snapshot_typed_projections(bytes) else {
            return;
        };
        if let Some(entry) = typed
            .iter()
            .find(|p| p.key == nmp_core::refs::REFS_PROFILE_KEY)
        {
            self.ref_profiles.apply_sidecar(
                &entry.payload,
                envelope.session_id,
                envelope.snapshot_epoch,
            );
        }
    }

    /// ADR-0063 (#1671 Lane F) — the rendered profile for `pubkey`, sourced from
    /// the kernel-pushed `refs.profile` row (the resolve_ref output), or `None`
    /// when no live ref is cached for that key. Shells render avatars/names from
    /// this; there is no second app-side profile cache (D4).
    #[must_use]
    pub fn profile(&self, pubkey: &str) -> Option<ProfileWire> {
        self.ref_profiles
            .profile(pubkey)
            .map(|card| crate::feature_snapshot_typed::profile_wire_from_card(pubkey, card))
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
        if tab == FeatureTab::Settings && self.settings_cursor == 0 {
            self.settings_cursor = 1;
        }
        self.status = format!("tab {}", tab.label());
    }

    pub fn next_tab(&mut self) {
        self.set_tab(self.tab.next());
    }

    pub fn previous_tab(&mut self) {
        self.set_tab(self.tab.previous());
    }

    pub fn open_palette(&mut self) {
        self.mode = Mode::Palette { cursor: 0 };
    }

    pub fn close_palette(&mut self) {
        if matches!(self.mode, Mode::Palette { .. }) {
            self.mode = Mode::Normal;
        }
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
            if result.status.eq_ignore_ascii_case("failed")
                || result.error.as_ref().is_some_and(|error| !error.is_empty())
            {
                self.push_toast(&message);
            }
            self.status = message;
            self.last_action_result = Some(result);
        }
        true
    }

    fn apply_feed_snapshot(&mut self, snapshot: Value) {
        // V-80 RootFeedSnapshot: `cards` is now `Vec<RootCard>` (one per thread
        // root), not the old flat event-card array. Each entry is one feed row.
        self.cards = snapshot
            .get("cards")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        self.timeline_has_more = snapshot
            .get("page")
            .and_then(|page| page.get("has_more"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
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
}

use crate::short_id;

#[cfg(test)]
#[path = "app/dynamic_feed_tests.rs"]
mod dynamic_feed_tests;

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

    #[test]
    fn failed_action_result_adds_toast() {
        let (runtime, _rx) = AppRuntime::new().expect("runtime starts without live relays");
        let mut state = AppState::default();

        let handled = state.apply_action_results(
            &runtime,
            vec![ActionResult {
                correlation_id: "abcdef1234567890fedcba".to_string(),
                status: "failed".to_string(),
                error: Some("blocked: duplicate group".to_string()),
            }],
        );

        assert!(handled);
        assert_eq!(
            state.status,
            "action abcdef12...fedcba failed: blocked: duplicate group"
        );
        assert_eq!(
            state.toasts.last().map(|(message, _)| message.as_str()),
            Some("action abcdef12...fedcba failed: blocked: duplicate group")
        );
    }
}
