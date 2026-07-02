//! Publish-outbox projection DTOs.
//!
//! Owns the user-facing snapshot rows for in-flight publish intents:
//! [`PublishOutboxItem`], its per-relay detail [`PublishOutboxRelay`], and the
//! [`OutboxSummarySnapshot`] counters. Derived from the publish engine's
//! in-flight snapshot; the shell never reconstructs retry policy or relay
//! state from logs.

use super::Serialize;

/// User-facing projection of publish intents that have not finished.
///
/// This is derived from the publish engine's in-flight snapshot; the UI never
/// reconstructs retry policy or relay state from logs.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct PublishOutboxItem {
    pub(super) handle: String,
    pub(super) event_id: String,
    pub(super) kind: u32,
    /// Raw verbatim content of the event being published. The shell formats
    /// this for display (truncation, encrypted-content placeholder, etc.).
    /// ADR-0072 / aim.md §2 #4: presentation formatting lives in the shell,
    /// not in the kernel. Replaces the removed `preview` / `title` /
    /// `system_image` pre-formatted fields.
    pub(super) content: String,
    /// Raw Unix-seconds creation timestamp. ADR-0072: projection sends raw
    /// epoch seconds; shells format for display with their own locale/TZ.
    /// Replaces the removed `created_at_display` wire field (V-115).
    pub(super) created_at: u64,
    pub(super) status: String,
    /// Pre-decided "is the Retry button enabled" flag. The kernel knows the
    /// retry-policy rule ("a row already sending cannot be retried"); the
    /// shell never reconstructs it. RMP bible commandment #4 — no native `if`
    /// deciding what the app should *do*.
    pub(super) can_retry: bool,
    pub(super) target_relays: usize,
    // ADR-0072 / V-115: `target_summary` removed — shells compose "N relays ·
    // <formatted time>" themselves from `target_relays` + `created_at`.
    pub(super) relays: Vec<PublishOutboxRelay>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct PublishOutboxRelay {
    pub(super) relay_url: String,
    pub(super) status: String,
    pub(super) attempt: u32,
    pub(super) message: String,
    /// Pre-formatted "why was this relay targeted?" string, computed by the
    /// outbox resolver at publish time and carried verbatim through the
    /// snapshot. Examples: `"NIP-65 write relay"`, `"App relay (local config)"`,
    /// `"Inbox relay for <hex pubkey>"` (raw hex — D6 forbids backend
    /// projections from calling `display::*` abbreviation helpers; the shell
    /// applies its own `short_npub` / bech32 rendering). Empty when the publish predates this
    /// projection field (older persisted rows) — `skip_serializing_if` keeps
    /// the JSON payload shape unchanged in that case so apps that don't yet
    /// read the field stay forward-compatible.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(super) relay_reason: String,
}

/// Outbox summary counters for `NotificationsView` (and similar shells).
/// The kernel owns the per-status counts; the shell derives any display
/// strings (headline, subtitle) from these raw counts using its own locale.
///
/// ADR-0072 / aim.md §2 #4: presentation formatting lives in the shell.
/// The previously-emitted `title` / `subtitle` pre-formatted English strings
/// have been removed; shells now compute them from the raw counters.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct OutboxSummarySnapshot {
    pub(super) total: u32,
    pub(super) sending: u32,
    pub(super) retrying: u32,
    pub(super) queued: u32,
    pub(super) failed: u32,
}
