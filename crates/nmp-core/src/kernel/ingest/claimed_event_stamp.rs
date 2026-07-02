//! ADR-0070 Rung 1, codex #1 condition 2 (F1) — the store-ingest chokepoint
//! that bumps `claimed_event_content_ver` when a freshly persisted event matches
//! a live `event_claims` key.
//!
//! Extracted from `ingest/mod.rs` (`verify_and_persist`) to keep that file at
//! its file-size baseline (AGENTS.md). Pure helper; no new state.

use super::super::{Kernel, NostrEvent};
use crate::store::InsertOutcome;

impl Kernel {
    /// Bump `claimed_event_content_ver` when `outcome` (from a `verify_and_persist`
    /// store insert) lands an event matching a live `event_claims` key — so the
    /// `refs.event` projection rev advances without waiting for a profile bump.
    ///
    /// `event_claims` keys (`requests/event.rs::primary_id`) are a hex64 event id
    /// (note claims), a `"kind:pubkey:d_tag"` coordinate (addressable /
    /// parameterized-replaceable claims), OR an `"i:<external-id>"` NIP-73
    /// external ref (#1654). ALL are checked on BOTH the `Inserted` and `Replaced`
    /// arms (F1): a kind:30023 longform arriving for the FIRST time returns
    /// `Inserted{id}` but is claimed by COORD, not by id — an id-only check would
    /// stall the rev and dark the embed; likewise a NIP-22 comment satisfying an
    /// external ref is claimed by its `i` tag, not by id.
    ///
    /// A single event may satisfy SEVERAL live claimed keys at once — its event-id
    /// key AND its coordinate key, or two distinct `["i", …]` external refs both
    /// claimed by different consumers. We therefore collect EVERY live key this
    /// insert satisfies and bump each per-key rev (codex lead-gate HIGH 2). An
    /// early `return` after the first match stalled the rev of every other claimed
    /// row the event satisfied, so those previews never re-rendered.
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
        let Some(id) = claimed_id else { return };

        // Collect EVERY live `event_claims` key this insert satisfies (the hex id,
        // the addressable coordinate, AND each NIP-73 external ref) so ADR-0070's
        // per-key rev can bump every affected row (D6a), not just the first match.
        let mut matched_keys: Vec<String> = Vec::new();

        let hex_id: String = id.iter().map(|b| format!("{b:02x}")).collect();
        if self.event_claims.contains_key(&hex_id) {
            matched_keys.push(hex_id);
        }
        // Addressable / parameterized-replaceable coord — applies to BOTH Inserted
        // (fresh) and Replaced (supersede).
        if crate::store::is_replaceable(event.kind) || crate::store::is_addressable(event.kind) {
            let d = event
                .tags
                .iter()
                .find(|t| t.first().map(|s| *s == "d").unwrap_or(false))
                .and_then(|t| t.get(1))
                .map(|s| s.as_str())
                .unwrap_or("");
            let coord_key = format!("{}:{}:{}", event.kind, event.pubkey, d);
            if self.event_claims.contains_key(&coord_key) {
                matched_keys.push(coord_key);
            }
        }
        // NIP-73 external refs (#1654): the event satisfies a claimed
        // `i:<external-id>` ref iff it carries a matching `["i", <external-id>]`
        // tag. Kind-agnostic — any kind may reference an external id, and an event
        // may tag SEVERAL external ids, each potentially claimed independently.
        for tag in &event.tags {
            if tag.len() >= 2 && tag[0] == "i" {
                let ext_key = format!("i:{}", tag[1]);
                if self.event_claims.contains_key(&ext_key) && !matched_keys.contains(&ext_key) {
                    matched_keys.push(ext_key);
                }
            }
        }

        if matched_keys.is_empty() {
            return;
        }
        self.projection_rev_tracker
            .source_versions
            .bump_claimed_event_content();
        // ADR-0070 Lane B (D6a) — per-key rev (ingest site 3 of 3): a freshly
        // persisted event rewrote THESE claimed rows' data; bump each one's rev so
        // every satisfied preview re-renders.
        for key in &matched_keys {
            self.projection_rev_tracker
                .source_versions
                .bump_event_row(key);
        }
    }
}
