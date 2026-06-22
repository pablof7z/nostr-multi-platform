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
///
/// Carries RAW kernel values only. Display strings (short URL, title-cased
/// role/connection/auth, compact event counts, formatted byte sizes,
/// discovery-kind labels) are derived in the shell render layer.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct RelayRow {
    #[serde(default)]
    pub relay_url: String,
    /// Raw role token (e.g. `"content"`, `"indexer"`). Shells title-case and
    /// derive their own hue.
    #[serde(default)]
    pub role: String,
    /// Raw connection token (e.g. `"connected"`). Shells title-case and derive
    /// their own hue.
    #[serde(default)]
    pub connection: String,
    /// Raw auth token (e.g. `"ok"`, `"—"`). Shells title-case (pass `"—"`) and
    /// derive their own hue.
    #[serde(default)]
    pub auth: String,
    #[serde(default)]
    pub total_sub_count: u64,
    #[serde(default)]
    pub active_sub_count: u64,
    #[serde(default)]
    pub eosed_sub_count: u64,
    #[serde(default)]
    pub total_events_rx: u64,
    #[serde(default)]
    pub reconnect_count: u64,
    /// Raw discovery kind numbers (deduplicated, sorted). Shells format.
    #[serde(default)]
    pub discovery_kinds: Vec<u64>,
    /// Raw bytes received. Shells format ("1.2 KB") when > 0.
    #[serde(default)]
    pub bytes_rx: u64,
    /// Raw bytes transmitted. Shells format ("1.2 KB") when > 0.
    #[serde(default)]
    pub bytes_tx: u64,
    /// Unix epoch milliseconds; 0 means "never observed". Raw value on the
    /// wire — shells format relative time at render.
    #[serde(default)]
    pub last_connected_ms: u64,
    #[serde(default)]
    pub last_event_ms: u64,
    #[serde(default)]
    pub last_notice: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub wire_subs: Vec<RelayWireSubRow>,
}

/// Wire-level subscription row within a relay's diagnostics.
///
/// Carries RAW kernel values only. Display strings (short wire id, title-cased
/// state, consumer-count phrase, compact event count) are derived in the shell
/// render layer.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct RelayWireSubRow {
    #[serde(default)]
    pub wire_id: String,
    #[serde(default)]
    pub relay_url: String,
    #[serde(default)]
    pub filter_summary: String,
    /// Raw state token (e.g. `"open"`). Shells title-case and derive their hue.
    #[serde(default)]
    pub state: String,
    /// Raw consumer count. Shells format as `"N consumer(s)"`.
    #[serde(default)]
    pub consumer_count: u32,
    /// Raw events received counter. Shells format as compact count.
    #[serde(default)]
    pub events_rx: u64,
    #[serde(default)]
    pub eose_observed: bool,
    /// Unix epoch milliseconds; 0 means "never observed". Raw value on wire.
    #[serde(default)]
    pub opened_ms: u64,
    #[serde(default)]
    pub last_event_ms: u64,
    #[serde(default)]
    pub eose_ms: u64,
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
    pub name: Option<String>,
    #[serde(default)]
    pub raw_display_name: Option<String>,
    #[serde(default)]
    pub display_name_camel: Option<String>,
    #[serde(default)]
    pub picture_url: Option<String>,
    #[serde(default)]
    pub banner: Option<String>,
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub nip05: String,
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub lud16: Option<String>,
    #[serde(default)]
    pub lud06: Option<String>,
    #[serde(default)]
    pub lnurl: Option<String>,
}
