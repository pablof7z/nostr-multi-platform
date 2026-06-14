//! ADR-0055 Rung 1, codex #1 condition 2 (F1) — the store-ingest chokepoint
//! that bumps `claimed_event_content_ver` when a freshly persisted event matches
//! a live `event_claims` key.
//!
//! Extracted from `ingest/mod.rs` (`verify_and_persist`) to keep that file at
//! its file-size baseline (AGENTS.md). Pure helper; no new state.

use super::super::{Kernel, NostrEvent};
use crate::store::InsertOutcome;

impl Kernel {
    /// Bump `claimed_event_content_ver` when `outcome` (from a `verify_and_persist`
    /// store insert) lands an event whose id OR addressable coord matches a live
    /// `event_claims` key — so the `claimed_events` projection rev advances
    /// without waiting for a profile bump.
    ///
    /// `event_claims` keys (`requests/event.rs::primary_id`) are either a hex64
    /// event id (note claims) OR a `"kind:pubkey:d_tag"` coordinate (addressable /
    /// parameterized-replaceable claims). BOTH are checked on BOTH the `Inserted`
    /// and `Replaced` arms (F1): a kind:30023 longform arriving for the FIRST time
    /// returns `Inserted{id}` but is claimed by COORD, not by id — an id-only
    /// check would stall the rev and dark the embed.
    pub(super) fn maybe_bump_claimed_event_content(
        &mut self,
        outcome: &InsertOutcome,
        event: &NostrEvent,
    ) {
        let claimed_id = match outcome {
            InsertOutcome::Inserted { id, .. } => Some(id),
            InsertOutcome::Replaced { new_id, .. } => Some(new_id),
            _ => None,
        };
        let should_bump = claimed_id.is_some_and(|id| {
            let hex_id: String = id.iter().map(|b| format!("{b:02x}")).collect();
            if self.event_claims.contains_key(&hex_id) {
                return true;
            }
            // Addressable / parameterized-replaceable coord fallback — applies to
            // BOTH Inserted (fresh) and Replaced (supersede).
            if crate::store::is_replaceable(event.kind)
                || crate::store::is_parameterized_replaceable(event.kind)
            {
                let d = event
                    .tags
                    .iter()
                    .find(|t| t.first().map(|s| *s == "d").unwrap_or(false))
                    .and_then(|t| t.get(1))
                    .map(|s| s.as_str())
                    .unwrap_or("");
                let coord_key = format!("{}:{}:{}", event.kind, event.pubkey, d);
                return self.event_claims.contains_key(&coord_key);
            }
            false
        });
        if should_bump {
            self.projection_rev_tracker
                .source_versions
                .bump_claimed_event_content();
        }
    }
}
