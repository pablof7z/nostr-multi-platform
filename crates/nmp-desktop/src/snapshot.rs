//! Read-only mirror of the kernel's JSON `KernelUpdate` envelope.
//!
//! Doctrine D0/D7: the UI owns *no* state beyond the latest snapshot. These
//! structs are a deserialization-only projection of the actor's emitted JSON
//! (see `nmp_core::kernel::types::KernelUpdate`). Every field is `#[serde(default)]`
//! so a forward-compatible kernel that adds/removes fields never breaks the
//! shell — best-effort rendering (D1).

use serde::Deserialize;

/// One timeline / thread row as projected by the kernel.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct TimelineItem {
    #[serde(default)]
    pub author_display: String,
    #[serde(default)]
    pub author_avatar_initials: String,
    /// `#rrggbb` deterministic colour the kernel assigns per author (D1).
    #[serde(default)]
    pub author_avatar_color: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub created_at_display: String,
    #[serde(default)]
    pub relay_count: u32,
}

/// Active-account / target profile card.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ProfileCard {
    #[serde(default)]
    pub display: String,
}

/// Per-relay connection health.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct RelayStatus {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub relay_url: String,
    #[serde(default)]
    pub connection: String,
}

/// Subset of the kernel metrics row we surface in the status bar.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Metrics {
    #[serde(default)]
    pub events_rx: u64,
    #[serde(default)]
    pub note_events: u64,
    #[serde(default)]
    pub visible_items: usize,
}

/// The latest decoded snapshot. Held behind a mutex and swapped wholesale on
/// every actor emit — the shell never mutates it.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub rev: u64,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub profile: ProfileCard,
    #[serde(default)]
    pub items: Vec<TimelineItem>,
    #[serde(default)]
    pub relay_statuses: Vec<RelayStatus>,
    #[serde(default)]
    pub metrics: Metrics,
    #[serde(default)]
    pub active_account: Option<serde_json::Value>,
    #[serde(default)]
    pub last_error_toast: Option<String>,
}

