//! Engine relay-lifecycle / I/O methods — resume, retry, persist, dispatch.
//!
//! Extracted from `engine.rs` to keep the orchestrator under the 500-LOC
//! hand-authored ceiling (AGENTS.md / V-12). This cluster owns:
//!   - the durable resume path (`resume_from_store`)
//!   - the relay-availability gate (`mark_relay_unavailable` / `mark_relay_available`)
//!   - the user-driven `retry_now` path
//!   - the internal `dispatch_pending*` helpers + `persist` write-through
//!
//! `on_ack`, `start_publish`, `tick`, and `cancel_publish` stay in `engine.rs`
//! because they own the state-machine progression itself; the methods here
//! are the I/O seams around it.

use std::collections::BTreeMap;

use super::super::action::{PublishHandle, RelayUrl};
use super::super::state::PerRelayState;
use super::super::traits::{PublishRecord, PublishStoreError, RelaySelectionReason};
use super::helpers;
use super::types::InFlight;
use super::{PublishEngine, PublishEngineError};

impl PublishEngine {
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
        }
        self.flush_view();
        Ok(())
    }

    /// Mark a relay as unavailable for publish delivery. Any event that was
    /// already `InFlight` to that relay moves back to durable `Pending` so a
    /// connection loss never consumes the publish intent.
    pub fn mark_relay_unavailable(
        &mut self,
        relay_url: &str,
        _now_ms: u64,
    ) -> Result<(), PublishEngineError> {
        let relay_url = helpers::canonical_relay_identity(relay_url);
        self.unavailable_relays.insert(relay_url.clone());
        let mut changed = Vec::new();
        for (handle, row) in &mut self.in_flight {
            let Some(state) = row.per_relay.get_mut(&relay_url) else {
                continue;
            };
            if matches!(state, PerRelayState::InFlight { .. }) {
                *state = PerRelayState::Pending;
                row.pending_retries.remove(&relay_url);
                row.dirty = true;
                changed.push(handle.clone());
            }
        }
        for handle in changed {
            self.persist(&handle)?;
        }
        self.flush_view();
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
        for handle in handles {
            self.dispatch_pending_for_relay(&handle, &relay_url, now_ms);
        }
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
