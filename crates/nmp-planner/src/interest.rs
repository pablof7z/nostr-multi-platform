//! `LogicalInterest`, `InterestShape`, and `NaddrCoord` types.
//!
//! A logical interest is what a kernel-side consumer (view, action, monitor,
//! sync job, or pointer loader) wants alive on the wire. The compiler in
//! `planner::compiler` turns N logical interests into M ≤ N per-relay plans.
//!
//! Design: `docs/design/subscription-compilation/intro.md` §2.1
//! Doctrine: D3 (outbox routing automatic), D6 (errors are internal Results),
//!           D8 (composite reverse index, zero per-event allocs after warmup).

// ─── Type aliases (lightweight; no nostr-sdk dep) ────────────────────────────

/// Hex-encoded 64-char pubkey.
pub type Pubkey = String;

/// Hex-encoded 64-char event id.
pub type EventId = String;

/// A `wss://` URL for a relay, re-exported from `nmp-relay-url` (Layer 0),
/// the single workspace authority for this alias (#2648).
pub use nmp_relay_url::RelayUrl;

/// Unix timestamp in seconds.
pub type UnixSeconds = u64;

/// A Nostr tag key (e.g. "e", "p", "t", "a").
pub type TagKey = String;

/// Maximum UTF-8 scalar count accepted for a relay NIP-50 search query.
///
/// This is a substrate safety bound, not product policy. Search modules/apps
/// remain free to reject or refine user-entered queries before they reach the
/// planner, but the planner never forwards unbounded text into a relay filter.
pub const MAX_SEARCH_QUERY_CHARS: usize = 256;

/// Normalize and bound a relay NIP-50 search query.
///
/// Empty / whitespace-only input is treated as absent. Non-empty input is
/// trimmed and truncated by Unicode scalar count so UTF-8 boundaries are never
/// split.
#[must_use]
pub fn bounded_search_query(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(MAX_SEARCH_QUERY_CHARS).collect())
}

// ─── Submodules (cohesive ownership) ─────────────────────────────────────────
//
// The interest vocabulary is split by ownership, each re-exported below so the
// public paths (`nmp_planner::interest::LogicalInterest`, etc.) stay stable:
//   • `coord`   — the `NaddrCoord` PRE address coordinate.
//   • `shape`   — `InterestShape` (normalised wire filter) + `PTagRouting`.
//   • `logical` — `LogicalInterest` and its identity/scope/lifecycle/hint parts.

mod coord;
mod logical;
mod shape;

pub use coord::NaddrCoord;
pub use logical::{
    HintSource, InterestId, InterestLifecycle, InterestScope, LogicalInterest, RelayHint,
};
pub use shape::{InterestShape, PTagRouting};

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "interest/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "interest/address_tests.rs"]
mod address_tests;
