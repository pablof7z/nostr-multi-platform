//! Accepted-event chokepoint for live relay and local-publish ingest.

use super::super::{Kernel, NostrEvent};
use super::IngestSource;

impl Kernel {
    /// ADR-0057 — the single kind-agnostic, source-agnostic accepted-event
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
    ///    read-cache is CALLED BY this one chokepoint post-`verify_and_persist`,
    ///    gated on the behavioral `follow_feed_kinds` predicate — not a scattered
    ///    per-kind/per-source ladder. The timeline read-cache is a chokepoint-fed
    ///    observer ([`Self::project_timeline_event`]) whose read-time relevance
    ///    predicate is `should_store_event` (which no longer has any power over
    ///    persistence).
    ///
    /// ADR-0057 PR 3 finishes D0: profiles (kind:0) AND contacts (kind:3) are
    /// both parser-fed (`nmp_nip01::Kind0Parser` / `Kind3Parser` writing the
    /// capability-owned `ProfileCache` / `ContactsCache`), detected via a
    /// before/after cache snapshot in [`Self::project_accepted_event`]. The
    /// ingest path now names ZERO NIP kind literals — the timeline projection is
    /// gated by the behavioral `follow_feed_kinds` predicate, and gift-wrap is
    /// excluded via the parser registry, not a literal.
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

        // Persistence ONLY: sig-verify -> store.insert -> raw-tap -> provenance
        // accounting -> TTL stamping (ADR-0057). Returns the verified clone so
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
            self.project_accepted_event(&verified);
            // Arm cache-serve wakeups for already-served interests matching this
            // event (#1520 — event-driven re-arm so live inserts surface in cache
            // projections without waiting for a full re-serve from the store).
            let raw = verified.raw();
            self.note_store_insert(
                &raw.id,
                &raw.pubkey,
                raw.kind,
                raw.created_at,
                &raw.tags,
            );
        }

        // Timeline read-cache projection — LIVE-path specific. The cache-serve
        // path has its own follow-set timeline append in `feed_served_event`.
        if self.follow_feed_kinds.contains(&event.kind) || sub_id.starts_with("diag-firehose-") {
            self.project_timeline_event(sub_id, &event, Some(&outcome));
        }

        Some(outcome)
    }
}
