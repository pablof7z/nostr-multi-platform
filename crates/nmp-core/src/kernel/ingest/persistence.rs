//! Signature verification, canonical store insertion, and persistence-side taps.

use super::super::{Kernel, NostrEvent};
use super::helpers;

impl Kernel {
    /// Verify and persist an event to the `EventStore` — persistence ONLY
    /// (ADR-0070: sig-verify -> store.insert -> raw-tap -> provenance accounting
    /// -> TTL stamping). The post-store projection fan-out (parser dispatch +
    /// transition sweep + clamped observer notify) is the caller's job via the
    /// shared [`Self::project_accepted_event`] helper.
    ///
    /// Returns `Some((outcome, verified))` when sig-verify succeeds — the
    /// `verified` clone lets the caller run projection without re-verifying —
    /// or `None` when signature verification (or the store insert) fails.
    /// Callers that perform local-cache mutations for replaceable kinds **must**
    /// inspect the outcome: only `Inserted | Replaced` means this event is now
    /// the canonical version in the store — all other outcomes must be treated
    /// as no-ops for cache purposes (D4).
    pub(super) fn verify_and_persist(
        &mut self,
        relay_url: &str,
        event: &NostrEvent,
    ) -> Option<(crate::store::InsertOutcome, crate::store::VerifiedEvent)> {
        let verified =
            match crate::store::VerifiedEvent::try_from_raw(helpers::raw_event_from_nostr(event)) {
                Ok(v) => v,
                Err(e) => {
                    self.log(format!(
                        "sig verify failed for {}: {e}",
                        helpers::event_short_id(&event.id)
                    ));
                    return None;
                }
            };
        // V-40 — clone the verified event for the substrate
        // [`EventIngestDispatcher`] fan-out. Cloning is cheap and lets us hand
        // `store.insert` an owned `VerifiedEvent` while still feeding parsers
        // AFTER the store gates supersession (D4).
        let verified_for_dispatch = verified.clone();
        // T105: store provenance is the *actual* URL the event came in on,
        // not the lane's bootstrap URL.
        let provenance = relay_url.to_string();
        // Clone the event into an `Arc` for the dispatcher ONLY when a sink
        // policy is actually active for this kind — the common no-sink / idle-kind
        // ingest path stays allocation-free. `verified` is moved into
        // `store.insert` below, so this conditional clone must happen here.
        let raw_arc: Option<std::sync::Arc<crate::store::RawEvent>> = self
            .external_event_sink_dispatcher()
            .filter(|dispatcher| !dispatcher.all_idle_for_kind(event.kind))
            .map(|_| std::sync::Arc::new(verified.raw().clone()));
        match self
            .store
            .insert(verified, &provenance, self.ingest_received_at_ms())
        {
            Ok(outcome) => {
                // Dispatch to external event sinks (replaces old raw tap).
                // `from_insert_outcome` preserves the DUPLICATE-inclusive outcome
                // gate from `raw_tap_should_fire` (Inserted | Replaced | Duplicate
                // | Ephemeral). `raw_arc` is `Some` only when a policy matched the
                // kind (the `all_idle_for_kind` fast-path above), so the frame is
                // built only when a sink will actually consume it.
                if let (Some(ingest_outcome), Some(raw_arc), Some(dispatcher)) = (
                    crate::substrate::IngestOutcomeKind::from_insert_outcome(&outcome),
                    raw_arc,
                    self.external_event_sink_dispatcher(),
                ) {
                    let source_relay: Option<std::sync::Arc<str>> =
                        Some(std::sync::Arc::from(provenance.as_str()));
                    if let Some(frame) = crate::substrate::SignedEventFrame::build(
                        raw_arc,
                        source_relay,
                        ingest_outcome,
                    ) {
                        dispatcher.dispatch(frame);
                    }
                }
                // T131 — per-URL `RelayUsefulness` provenance accounting.
                match &outcome {
                    crate::store::InsertOutcome::Inserted { .. } => {
                        self.event_provenance
                            .record_first_source(&event.id, &provenance);
                    }
                    crate::store::InsertOutcome::Replaced { .. } => {
                        self.event_provenance.record_replaced(&provenance);
                    }
                    crate::store::InsertOutcome::Duplicate { .. } => {
                        self.event_provenance.record_duplicate(&provenance);
                    }
                    crate::store::InsertOutcome::Rejected { .. } => {
                        self.event_provenance.record_rejected(&provenance);
                    }
                    crate::store::InsertOutcome::Superseded { .. }
                    | crate::store::InsertOutcome::Tombstoned { .. }
                    | crate::store::InsertOutcome::Ephemeral { .. } => {}
                }

                // F-TTL — replaceable/addressable event freshness hook.
                let is_regular = crate::store::is_replaceable(event.kind);
                let is_addressable = crate::store::is_addressable(event.kind);
                if is_regular || is_addressable {
                    if let Some(pubkey_bytes) = crate::kernel::hex_to_pubkey_bytes(&event.pubkey) {
                        let key = if is_addressable {
                            let d_tag = event
                                .tags
                                .iter()
                                .find(|t| t.first().map(|s| s == "d").unwrap_or(false))
                                .and_then(|t| t.get(1))
                                .cloned()
                                .unwrap_or_default();
                            crate::store::ReplaceableKey::Parameterized {
                                kind: event.kind,
                                pubkey: pubkey_bytes,
                                d_tag,
                            }
                        } else {
                            crate::store::ReplaceableKey::Regular {
                                kind: event.kind,
                                pubkey: pubkey_bytes,
                            }
                        };
                        let ttl_ms =
                            self.replaceable_ttl.ttl_for_kind(event.kind).as_millis() as u64;
                        self.store
                            .set_check_again_after(key, self.now_ms() + ttl_ms);
                    }
                }

                self.maybe_bump_claimed_event_content(&outcome, event); // ADR-0070 (F1)
                Some((outcome, verified_for_dispatch))
            }
            Err(e) => {
                self.log(format!(
                    "store insert error for {}: {e}",
                    helpers::event_short_id(&event.id)
                ));
                None
            }
        }
    }
}
