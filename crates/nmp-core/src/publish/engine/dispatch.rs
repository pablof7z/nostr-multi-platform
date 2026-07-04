//! Engine relay-lifecycle / I/O methods — resume, retry, persist, dispatch.
//!
//! Extracted from `engine.rs` to keep the orchestrator under the 500-LOC
//! hand-authored ceiling (AGENTS.md / V-12). This cluster owns:
//!   - the durable resume path (`resume_from_store`)
//!   - the relay-availability gate (`mark_relay_unavailable` / `mark_relay_available`)
//!   - the user-driven `retry_now` and `cancel_by_handle` paths
//!   - the internal `dispatch_pending*` helpers + `persist` write-through
//!
//! `on_ack`, `start_publish`, and `tick` stay in `engine.rs` because they own
//! the state-machine progression itself; the methods here are the I/O seams
//! around it. `cancel_by_handle` (S7/#1754) lives here as the cancel I/O seam,
//! analogous to `retry_now`.

use std::collections::BTreeMap;

use super::super::action::{PublishHandle, RelayUrl};
use super::super::state::PerRelayState;
use super::super::traits::{PublishRecord, PublishStoreError, RelaySelectionReason};
use super::helpers;
use super::types::{InFlight, LastTerminal};
use super::{PublishEngine, PublishEngineError};

impl PublishEngine {
    /// Cancel an in-flight publish addressed by its `handle` (== signed event
    /// id), recording the `Cancelled` terminal under `correlation_id`.
    ///
    /// S7 (#1754): this is invoked DIRECTLY by the kernel's cancel-by-id doorway
    /// — there is no longer a `PublishAction::Cancel` routed through
    /// `start_publish`. The caller resolves the original dispatch
    /// `correlation_id` from the durable handle↔correlation index and passes it
    /// here, so the terminal lands under the id the host's spinner is bound to
    /// (PD-036), not the handle/event id. For an internal publish with no
    /// distinct dispatch id the caller passes the handle as the
    /// `correlation_id`, preserving the prior behaviour.
    ///
    /// INFALLIBLE (D6): the `Cancelled` terminal is recorded BEFORE the
    /// best-effort durable delete, so a store-delete failure can never orphan the
    /// host spinner — the in-memory cancel and the terminal always land.
    pub(crate) fn cancel_by_handle(
        &mut self,
        handle: &PublishHandle,
        correlation_id: &str,
        now_ms: u64,
    ) {
        if let Some(mut row) = self.in_flight.remove(handle) {
            self.needs_in_flight_rebuild = true;
            for state in row.per_relay.values_mut() {
                if !state.is_terminal() {
                    *state = PerRelayState::FailedAfterRetries {
                        reason: "cancelled".to_string(),
                        last_at_ms: now_ms,
                    };
                }
            }
        }
        // Record the cancelled terminal FIRST — BEFORE the best-effort durable
        // delete — so the host spinner is ALWAYS resolved under the original
        // correlation_id, even if the store delete fails (D6: a durable-cleanup
        // failure is not a reason to orphan the user's cancel). It is recorded on
        // the single `pending_terminals` engine→ledger stream (drained into
        // `action_results` / `action_lifecycle` as the distinct `cancelled` stage,
        // AND — via its `PublishQueueTerminal::Cancelled` payload — into the
        // event-id-keyed `publish_queue` row), even for an unknown / already-settled
        // handle: it is a terminal verdict the host asked for.
        self.record_terminal(LastTerminal {
            // S7 (#1754): the terminal correlation_id is the ORIGINAL dispatch
            // id, never the handle — this is the PD-036 fix.
            correlation_id: correlation_id.to_string(),
            status: "cancelled",
            error: None,
            // Cancel concerns a signed event; surface its id (#1702).
            event_id: Some(handle.clone()),
            result_json: None,
            reason_code: None,
            // S11 slice 4 (#1758): the SAME terminal flips the event-id-keyed
            // queue row to `"cancelled"` (empty per-relay outcomes) — the kernel's
            // ledger fold drives `set_publish_entry_terminal` from here, replacing
            // the prior explicit cancel-path `set_publish_entry_terminal` call. The
            // queue is keyed by the resolved publish `handle` (the prior cancel-path
            // key), carried explicitly so the kernel never unwraps `event_id`.
            publish_queue: super::types::PublishQueueTerminal::Cancelled {
                event_id: handle.clone(),
            },
        });
        // Best-effort durable cleanup. A delete failure (or a not-found row)
        // is a silent no-op: the in-memory cancel and the terminal already
        // landed; a stale durable row is harmless and gets pruned by the next
        // resume sweep. D6 — the store error never propagates out of cancel.
        let _ = self.store.delete(handle);
        self.flush_view();
    }

    /// Resume any pending records left by a prior process. Called once at
    /// kernel boot. M3 LMDB will return real rows; the in-memory shim returns
    /// what was previously upserted.
    ///
    /// Restores `pending_retries` from the persisted record so a mid-backoff
    /// state survives restart with its scheduled retry deadline intact —
    /// `dispatch_pending` will fire the retry only when `now_ms` reaches the
    /// stored deadline (no thundering herd, no silent drop). When the record
    /// has no `pending_retries` entry for a relay in `RelayError`/`TimedOut`
    /// (older serialised rows), `dispatch_due` falls back to retry-now so the
    /// resume path stays best-effort.
    pub fn resume_from_store(&mut self, now_ms: u64) -> Result<(), PublishEngineError> {
        for record in self.store.load_pending()? {
            let mut per_relay = BTreeMap::new();
            for (url, state) in record.per_relay {
                per_relay.insert(helpers::canonical_relay_identity(&url), state);
            }
            let mut pending_retries = BTreeMap::new();
            for (url, due_ms) in record.pending_retries {
                pending_retries.insert(helpers::canonical_relay_identity(&url), due_ms);
            }
            // Restore the per-relay selection rationale alongside the state
            // map. Older serialised rows (`relay_reasons` defaulted to empty)
            // simply project with an empty string per relay — the projection
            // skips empty `relay_reason` fields via `skip_serializing_if`.
            let mut relay_reasons: BTreeMap<RelayUrl, Vec<RelaySelectionReason>> = BTreeMap::new();
            for (url, reasons) in record.relay_reasons {
                relay_reasons.insert(helpers::canonical_relay_identity(&url), reasons);
            }
            let in_flight = InFlight {
                event: record.event,
                per_relay,
                relay_reasons,
                pending_retries,
                dirty: true,
                // A resumed publish survived a process restart; the minted
                // correlation_id was process-scoped and the host that issued
                // the dispatch is gone. The terminal verdict falls back to the
                // handle — the same id a non-dispatch publish would report.
                correlation_id_override: None,
            };
            self.in_flight.insert(record.handle.clone(), in_flight);
            self.dispatch_pending(&record.handle, now_ms);
            // A resumed row whose every relay is settled terminal by the
            // dispatch (e.g. the D10 emit gate refused a persisted private
            // envelope targeting public relays, settling each FailedAfterRetries
            // without an `on_ack`) MUST be terminally finalized + deleted from
            // the durable store here — otherwise it lingers Pending and is
            // re-refused on every subsequent resume (lingering debt). Reuses the
            // single complete-row finalization path `tick` uses.
            self.finalize_completed_rows(std::slice::from_ref(&record.handle), now_ms);
        }
        self.flush_view();
        Ok(())
    }

    /// Mark a relay as unavailable for publish delivery — a genuine
    /// connectivity loss (socket dial failed, mid-session drop, or an
    /// outbound frame evicted before it reached the transport). Any event
    /// that was already `InFlight` to that relay moves back to durable
    /// `Pending` so a connection loss never consumes the publish intent.
    ///
    /// BOUNDED (#2967): records (or preserves) the wall-clock moment this
    /// relay went unavailable so [`super::helpers::sweep_unavailable_timeouts`]
    /// can force-settle any row still parked behind it once
    /// `policy.inflight_deadline_ms` elapses — a persistently unreachable
    /// relay in a multi-relay publish set must not block the whole handle
    /// from completing forever. An already-recorded bounded timestamp is
    /// preserved (not reset) across repeated `Failed` events for the same
    /// relay, since the pool retries + fails a dead relay on its own
    /// reconnect cadence — the deadline is measured from the FIRST failure.
    pub fn mark_relay_unavailable(
        &mut self,
        relay_url: &str,
        now_ms: u64,
    ) -> Result<(), PublishEngineError> {
        let relay_url = helpers::canonical_relay_identity(relay_url);
        match self.unavailable_relays.get(&relay_url) {
            Some(Some(_)) => {}
            _ => {
                self.unavailable_relays
                    .insert(relay_url.clone(), Some(now_ms));
            }
        }
        self.demote_inflight_to_pending(&relay_url)?;
        self.flush_view();
        Ok(())
    }

    /// Park a relay awaiting NIP-42 re-authentication (Finding B). Shares the
    /// InFlight→Pending demotion `mark_relay_unavailable` uses, but records an
    /// UNBOUNDED (`None`) entry: the challenge→sign→AUTH→OK round-trip can
    /// legitimately take several seconds with a remote (NIP-46) signer, and
    /// #2967's fail-fast deadline must never settle a false
    /// `FailedAfterRetries` mid-handshake. An already-bounded entry (a
    /// concurrent genuine connectivity loss on the same relay) is left alone
    /// — the socket being verifiably down takes precedence over an auth park
    /// that could not otherwise be happening. Only `mark_relay_available`
    /// (the relay reaching `Authenticated`) clears this.
    pub(super) fn park_relay_awaiting_auth(
        &mut self,
        relay_url: &str,
        _now_ms: u64,
    ) -> Result<(), PublishEngineError> {
        let relay_url = helpers::canonical_relay_identity(relay_url);
        self.unavailable_relays
            .entry(relay_url.clone())
            .or_insert(None);
        self.demote_inflight_to_pending(&relay_url)?;
        self.flush_view();
        Ok(())
    }

    /// Shared InFlight→Pending demotion for both [`Self::mark_relay_unavailable`]
    /// and [`Self::park_relay_awaiting_auth`] — the two differ only in how they
    /// record `relay_url` into `unavailable_relays`, never in how an in-flight
    /// send is walked back.
    fn demote_inflight_to_pending(&mut self, relay_url: &str) -> Result<(), PublishEngineError> {
        let mut changed = Vec::new();
        for (handle, row) in &mut self.in_flight {
            let Some(state) = row.per_relay.get_mut(relay_url) else {
                continue;
            };
            if matches!(state, PerRelayState::InFlight { .. }) {
                *state = PerRelayState::Pending;
                row.pending_retries.remove(relay_url);
                row.dirty = true;
                changed.push(handle.clone());
            }
        }
        for handle in changed {
            self.persist(&handle)?;
        }
        Ok(())
    }

    /// Mark a relay as available and immediately dispatch any pending intent
    /// targeted at that relay. This is the connection/reconnection sync path;
    /// regular retry ticks also use the same availability gate.
    pub fn mark_relay_available(
        &mut self,
        relay_url: &str,
        now_ms: u64,
    ) -> Result<(), PublishEngineError> {
        let relay_url = helpers::canonical_relay_identity(relay_url);
        self.unavailable_relays.remove(&relay_url);
        let handles: Vec<PublishHandle> = self.in_flight.keys().cloned().collect();
        for handle in &handles {
            self.dispatch_pending_for_relay(handle, &relay_url, now_ms);
        }
        // Finalize any row the dispatch left fully terminal (D10 refusal of a
        // private envelope on this relay) so it is settled + removed, not left
        // pending. Same path `tick` / resume use.
        self.finalize_completed_rows(&handles, now_ms);
        self.flush_view();
        Ok(())
    }

    /// User-requested immediate retry for a pending publish. This does not
    /// override relay availability: unavailable relays stay durable Pending
    /// until their socket reconnects, but pending/backoff states for available
    /// relays are eligible to dispatch now.
    pub fn retry_now(
        &mut self,
        handle: &PublishHandle,
        now_ms: u64,
    ) -> Result<(), PublishEngineError> {
        let Some(row) = self.in_flight.get_mut(handle) else {
            return Err(PublishEngineError::Store(PublishStoreError::NotFound));
        };
        for (relay_url, state) in &row.per_relay {
            if !state.is_terminal() {
                row.pending_retries.remove(relay_url);
            }
        }
        row.dirty = true;
        self.persist(handle)?;
        self.dispatch_pending(handle, now_ms);
        // Finalize if the dispatch left the row fully terminal (D10 refusal of a
        // private envelope), so a manual retry settles + removes it rather than
        // leaving it pending to be re-refused. Same path `tick` / resume use.
        self.finalize_completed_rows(std::slice::from_ref(handle), now_ms);
        self.flush_view();
        Ok(())
    }

    pub(super) fn dispatch_pending(&mut self, handle: &PublishHandle, now_ms: u64) {
        self.dispatch_pending_matching(handle, None, now_ms);
    }

    pub(super) fn dispatch_pending_for_relay(
        &mut self,
        handle: &PublishHandle,
        relay_url: &str,
        now_ms: u64,
    ) {
        self.dispatch_pending_matching(handle, Some(relay_url), now_ms);
    }

    fn dispatch_pending_matching(
        &mut self,
        handle: &PublishHandle,
        relay_filter: Option<&str>,
        now_ms: u64,
    ) {
        let Some(in_flight) = self.in_flight.get_mut(handle) else {
            return;
        };
        let frame = helpers::build_event_frame(&in_flight.event);
        let acks = helpers::dispatch_due(
            in_flight,
            now_ms,
            &*self.dispatcher,
            &frame,
            relay_filter,
            &self.unavailable_relays,
        );
        for ack in acks {
            self.on_ack(handle, ack, now_ms);
        }
    }

    pub(super) fn persist(&self, handle: &PublishHandle) -> Result<(), PublishEngineError> {
        let Some(in_flight) = self.in_flight.get(handle) else {
            return Ok(());
        };
        let record = PublishRecord {
            handle: handle.clone(),
            event: in_flight.event.clone(),
            per_relay: in_flight
                .per_relay
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            // Persist scheduled retry deadlines so a restart mid-backoff
            // resumes with the same wait, not a thundering retry.
            pending_retries: in_flight
                .pending_retries
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect(),
            // Persist per-relay rationale so the human-readable
            // "why was this relay targeted?" string survives kernel restart
            // and is available to the snapshot projection without re-running
            // the resolver.
            relay_reasons: in_flight
                .relay_reasons
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        };
        self.store.upsert(&record).map_err(PublishEngineError::from)
    }
}
