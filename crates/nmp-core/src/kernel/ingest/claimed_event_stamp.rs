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
    /// `event_claims` keys (`requests/event.rs::primary_id`) are a hex64 event id
    /// (note claims), a `"kind:pubkey:d_tag"` coordinate (addressable /
    /// parameterized-replaceable claims), OR an `"i:<external-id>"` NIP-73
    /// external ref (#1654). ALL are checked on BOTH the `Inserted` and `Replaced`
    /// arms (F1): a kind:30023 longform arriving for the FIRST time returns
    /// `Inserted{id}` but is claimed by COORD, not by id — an id-only check would
    /// stall the rev and dark the embed; likewise a NIP-22 comment satisfying an
    /// external ref is claimed by its `i` tag, not by id.
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
        // Resolve WHICH live `event_claims` key this insert satisfies (the hex id,
        // the addressable coordinate, OR a NIP-73 external ref) so ADR-0063's
        // per-key rev can bump that exact row (D6a), not just the whole-projection
        // scalar.
        let matched_key: Option<String> = claimed_id.and_then(|id| {
            let hex_id: String = id.iter().map(|b| format!("{b:02x}")).collect();
            if self.event_claims.contains_key(&hex_id) {
                return Some(hex_id);
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
                if self.event_claims.contains_key(&coord_key) {
                    return Some(coord_key);
                }
            }
            // NIP-73 external-ref fallback (#1654): the event satisfies a claimed
            // `i:<external-id>` ref iff it carries a matching `["i", <external-id>]`
            // tag. Kind-agnostic — any kind may reference an external id.
            for tag in &event.tags {
                if tag.len() >= 2 && tag[0] == "i" {
                    let ext_key = format!("i:{}", tag[1]);
                    if self.event_claims.contains_key(&ext_key) {
                        return Some(ext_key);
                    }
                }
            }
            None
        });
        if let Some(key) = matched_key {
            self.projection_rev_tracker
                .source_versions
                .bump_claimed_event_content();
            // ADR-0063 Lane B (D6a) — per-key rev (ingest site 3 of 3): a freshly
            // persisted event rewrote THIS claimed row's data; bump only its rev.
            self.projection_rev_tracker
                .source_versions
                .bump_event_row(&key);
        }
    }
}
