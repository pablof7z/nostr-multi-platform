//! Accepted-event chokepoint for live relay and local-publish ingest.

use super::super::{Kernel, NostrEvent};
use super::IngestSource;

impl Kernel {
    /// ADR-0070 — the single kind-agnostic, source-agnostic accepted-event
    /// chokepoint. Replaces the two hand-maintained per-kind ingest ladders
    /// (`handle_event`'s relay `match event.kind` arms and the deleted
    /// `record_local_publish_intent` mirror arms).
    ///
    /// Three concerns are now three layers:
    ///
    /// 1. **Admission** = valid signature only — enforced inside
    ///    [`Self::verify_and_persist`] (`try_from_raw`). No relevance gate, no
    ///    acquisition-match, no kind gate.
    /// 2. **Delivery vs persistence** = gated by the store [`InsertOutcome`].
    ///    `verify_and_persist` does persistence ONLY (`Inserted | Replaced`;
    ///    ephemerals return `Ephemeral` un-stored) and returns the
    ///    `(InsertOutcome, VerifiedEvent)`. The shared
    ///    [`Self::project_accepted_event`] then fires the NIP-parser dispatch +
    ///    the app-facing [`Kernel::notify_event_observers`] seam on the canonical
    ///    accepted outcome (`Inserted | Replaced | Ephemeral`). `Duplicate` (incl. a
    ///    relay echo of a locally-published event) is projection-silent —
    ///    preserving D4 single-fire for read-your-writes.
    /// 3. **Projection / relevance** = read-time only. The kernel-owned timeline
    ///    read-cache is an internal diagnostic/read-model observer
    ///    ([`Self::project_timeline_event`]) whose read-time relevance predicate
    ///    is `should_store_event` (which no longer has any power over
    ///    persistence).
    ///
    /// Profiles are parser-fed into the profile cache. Contacts are read
    /// through the protocol-owned contact-list reader:
    /// [`Self::project_accepted_event`] derives the active-account transition
    /// from that registered reader and enqueues the source recompile trigger.
    /// Gift-wrap is excluded via the parser registry, not a literal.
    ///
    /// Returns the store outcome so a source wrapper can apply source-specific
    /// post-processing (e.g. the relay path's claim-hit scoring).
    pub(in crate::kernel) fn ingest_accepted_event(
        &mut self,
        source: IngestSource<'_>,
        event: NostrEvent,
    ) -> Option<crate::store::InsertOutcome> {
        use crate::store::InsertOutcome;

        let provenance = source.provenance();
        let sub_id = source.sub_id();
        let active_contacts_before = self.active_contacts_snapshot_for_author(&event.pubkey);

        // Persistence ONLY: sig-verify -> store.insert -> raw-tap -> provenance
        // accounting -> TTL stamping (ADR-0070). Returns the verified clone so
        // the shared projection helper runs without re-verifying the signature.
        let Some((outcome, verified)) = self.verify_and_persist(provenance, &event) else {
            return None;
        };

        // The single shared post-store projection fan-out (parser dispatch +
        // capability-cache transition sweep + D9-clamped app-observer notify),
        // gated on the canonical accepted outcome.
        let canonical = matches!(
            outcome,
            InsertOutcome::Inserted { .. }
                | InsertOutcome::Replaced { .. }
                | InsertOutcome::Ephemeral { .. }
        );
        if canonical {
            self.project_accepted_event_from(
                &verified,
                source.parser_source_relay_url(),
                active_contacts_before,
            );
            // Keep the read-cache (`self.events` / `self.timeline`) consistent with
            // the store's current head when a replaceable event is replaced live.
            // Without this, a stale predecessor that cache-serve previously served
            // into the read-cache lingers, so the #1520 wakeup re-serve below sees
            // the new head as "uncached" and re-feeds it — a DUPLICATE
            // `project_accepted_event` fan-out that violates D4 single-fire (it
            // re-notifies observers for an event already delivered live).
            self.reconcile_read_cache_on_replace(&outcome, &verified);
            let relay_count = match &outcome {
                InsertOutcome::Inserted { sources_after, .. } => Some(*sources_after),
                InsertOutcome::Replaced { .. } => Some(1),
                _ => None,
            };
            if let Some(relay_count) = relay_count {
                self.cache_event_for_matching_open_interest(&event, relay_count);
            }
            // Arm BOTH store-wakeup arms for this mutation (ADR-0072 §10): the
            // #1520 cache-serve re-arm for already-served interests matching this
            // event (so live inserts surface in cache projections without a full
            // re-serve), and the pull-cursor wake for any registered cursor whose
            // `after_seq` is now behind the advanced ingest log.
            let raw = verified.raw();
            // The ingest log advances only for stored events (Inserted | Replaced);
            // ephemerals are never stored, so they must not arm a pull wake.
            let store_log_advanced = matches!(
                outcome,
                InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. }
            );
            self.note_store_mutation(
                &raw.id,
                &raw.pubkey,
                raw.kind,
                raw.created_at,
                &raw.tags,
                store_log_advanced,
            );
        }

        // Timeline read-cache projection — live diagnostic/test path specific.
        // Production feed sessions render through registered observers and
        // pull controllers; the core no longer owns an active-follows kind gate.
        if sub_id.starts_with("diag-firehose-") {
            self.project_timeline_event(sub_id, &event, Some(&outcome));
        }

        Some(outcome)
    }

    /// Keep the kernel read-cache (`self.events` + `self.timeline`) consistent
    /// with the store's current replaceable head after a live `Replaced`.
    ///
    /// The store evicts the superseded predecessor on insert (NIP-01 replaceable
    /// semantics), but the read-cache is populated independently — for
    /// the timeline projection, and for ANY kind by the cache-serve replay
    /// (`feed_served_event`). When a previously-served event
    /// (e.g. a kind:3 contact list served at interest registration) is replaced
    /// live, its predecessor must be removed from the read-cache and the new head
    /// recorded — otherwise the read-cache disagrees with the store.
    ///
    /// This is also what keeps the #1520 cache-serve wakeup single-fire: the
    /// wakeup re-serves the interest, and `serve_chunk` skips events already in
    /// `self.events`. If the predecessor lingered (and the new head were absent),
    /// the re-serve would treat the new head as uncached and re-feed it through
    /// `project_accepted_event`, re-notifying observers for an event already
    /// delivered by the live path (a D4 single-fire violation).
    ///
    /// Scope-limited to entries the read-cache ALREADY holds: the swap only runs
    /// when `self.events` contains the replaced id, so no new kinds enter the
    /// read-cache and cold-start serves (empty `self.events`) are untouched.
    fn reconcile_read_cache_on_replace(
        &mut self,
        outcome: &crate::store::InsertOutcome,
        verified: &crate::store::VerifiedEvent,
    ) {
        let crate::store::InsertOutcome::Replaced { replaced_id, .. } = outcome else {
            return;
        };
        let replaced_hex: String = replaced_id.iter().map(|b| format!("{b:02x}")).collect();
        // Only repair entries cache-serve / the timeline projection already
        // created — never introduce a brand-new read-cache entry here.
        if self.events.remove(&replaced_hex).is_none() {
            return;
        }
        // Drop the stale predecessor from the ordered timeline too (if present).
        if let Some(pos) = self.timeline.iter().position(|id| id == &replaced_hex) {
            self.timeline.remove(pos);
        }
        // Record the new head so the wakeup re-serve dedups it (no double-fire).
        // Mirrors `feed_served_event`'s read-cache entry (raw `created_at`, no
        // relay confirmation in-session → `relay_count: 0`).
        let raw = verified.raw();
        self.events.insert(
            raw.id.clone(),
            super::super::types::StoredEvent {
                id: raw.id.clone(),
                author: raw.pubkey.clone(),
                kind: raw.kind,
                created_at: raw.created_at,
                tags: raw.tags.clone(),
                content: raw.content.clone(),
                relay_count: 0,
            },
        );
        self.cached_estimated_store_bytes.set(None);
    }
}
