//! Shared snapshot types for Chirp shells.
//!
//! Per plan-chirp-cross-platform.md §2.2, these types are declared once here
//! rather than re-declared per shell. Both desktop and TUI shells consume
//! these definitions from `nmp-app-chirp`.
//!
//! Doctrine: all types are deserialization-only, forward-compatible projections.
//! Every field carries `#[serde(default)]` so the kernel can add/remove fields
//! without breaking shells.

use serde::Deserialize;

/// Snapshot of runtime metrics shared across shells.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct RuntimeMetrics {
    #[serde(default)]
    pub events_rx: u64,
    #[serde(default)]
    pub visible_items: u64,
    #[serde(default)]
    pub actor_queue_depth: u64,
    #[serde(default)]
    pub update_sequence: u64,
}

/// Relay connection row in diagnostics projection (relay_diagnostics).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct RelayRow {
    #[serde(default)]
    pub relay_url: String,
    #[serde(default)]
    pub short_url: String,
    #[serde(default)]
    pub role_label: String,
    #[serde(default)]
    pub role_tone: String,
    #[serde(default)]
    pub connection_label: String,
    #[serde(default)]
    pub connection_tone: String,
    #[serde(default)]
    pub auth_label: String,
    #[serde(default)]
    pub auth_tone: String,
    #[serde(default)]
    pub total_sub_count: u64,
    #[serde(default)]
    pub active_sub_count: u64,
    #[serde(default)]
    pub eosed_sub_count: u64,
    #[serde(default)]
    pub total_events_rx: u64,
    #[serde(default)]
    pub total_events_display: String,
    #[serde(default)]
    pub reconnect_count: u64,
    #[serde(default)]
    pub bytes_rx_display: Option<String>,
    #[serde(default)]
    pub bytes_tx_display: Option<String>,
    #[serde(default)]
    pub last_connected_display: Option<String>,
    #[serde(default)]
    pub last_event_display: Option<String>,
    #[serde(default)]
    pub last_notice: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub wire_subs: Vec<RelayWireSubRow>,
}

/// Wire-level subscription row within a relay's diagnostics.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct RelayWireSubRow {
    #[serde(default)]
    pub wire_id: String,
    #[serde(default)]
    pub short_wire_id: String,
    #[serde(default)]
    pub relay_url: String,
    #[serde(default)]
    pub filter_summary: String,
    #[serde(default)]
    pub state_label: String,
    #[serde(default)]
    pub state_tone: String,
    #[serde(default)]
    pub consumer_count_label: String,
    #[serde(default)]
    pub events_rx_display: Option<String>,
    #[serde(default)]
    pub eose_observed: bool,
    #[serde(default)]
    pub opened_display: String,
    #[serde(default)]
    pub last_event_display: Option<String>,
    #[serde(default)]
    pub eose_display: Option<String>,
    #[serde(default)]
    pub close_reason: Option<String>,
}

/// Interest (filter) row in diagnostics projection (relay_diagnostics).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct InterestRow {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub refcount: u64,
    #[serde(default)]
    pub cache_coverage: String,
}

/// Action result snapshot from action_results projection.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct ActionResult {
    #[serde(default)]
    pub correlation_id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
}

/// Action stage row from action_stages projection.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct ActionStageRow {
    #[serde(default)]
    pub correlation_id: String,
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub reason: Option<String>,
}

/// User profile card snapshot (snapshot.profile field).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ProfileCard {
    #[serde(default)]
    pub pubkey: String,
    #[serde(default)]
    pub npub: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub picture_url: Option<String>,
    #[serde(default)]
    pub nip05: String,
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub has_profile: bool,
    #[serde(default)]
    pub lnurl: Option<String>,
}
