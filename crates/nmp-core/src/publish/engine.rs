//! `PublishEngine` — the orchestrator that ties action, state, traits, and
//! view together.
//!
//! Single-threaded by design: the kernel actor (M6 ledger) drives it via
//! `start_publish` / `on_ack` / `tick`. Time is injected (`now_ms`) so the
//! engine is deterministic in tests; the actor passes `Instant::now()` in
//! production.
//!
//! The engine never spawns threads, never touches sockets, and never panics.
//! Two kinds of failure paths exist, and both honour D6 (errors never cross
//! FFI as exceptions):
//!
//! - **Per-relay relay-side failures** surface as `RecentFailure` rows on the
//!   snapshot (via `apply_verdict` → `FailedAfterRetries`) and as
//!   `PublishOutcome::Mixed` / `FailedAfterRetries` on the action ledger.
//! - **Engine-level failures** (`PublishEngineError::DuplicateHandle`,
//!   `NoTargets`, `Store`) are returned through the in-process `Result` so
//!   the actor can branch on them, then mapped via
//!   `engine::error_mapping::engine_error_to_failure` into a `RecentFailure`
//!   row on the same snapshot before the boundary crosses to Swift / Kotlin.

mod error_mapping;
mod helpers;
#[cfg(test)]
mod tests;

pub use error_mapping::{engine_error_to_failure, ENGINE_FAILURE_RELAY_URL};
pub use helpers::outcome_of;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::action::{PublishAction, PublishHandle, PublishTarget, RelayUrl};
use super::state::{apply_ack, classify_ack, AckClass, PerRelayState, RelayAck, RetryPolicy};
use super::traits::{
    OutboxResolver, PublishRecord, PublishStore, PublishStoreError, RelayDispatcher, Signer,
};
use super::view::{EventPublishStatus, PublishStatusSnapshot, PublishStatusState, RecentFailure};
use crate::substrate::SignedEvent;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PublishEngineError {
    DuplicateHandle(PublishHandle),
    NoTargets,
    Store(PublishStoreError),
}

impl From<PublishStoreError> for PublishEngineError {
    fn from(err: PublishStoreError) -> Self {
        Self::Store(err)
    }
}

/// One in-flight publish row owned by the engine.
pub(super) struct InFlight {
    pub event: SignedEvent,
    pub per_relay: BTreeMap<RelayUrl, PerRelayState>,
    pub pending_retries: BTreeMap<RelayUrl, u64>, // relay -> earliest retry epoch ms
    pub dirty: bool,
}

/// T128: terminal verdict for a settled publish. The engine records one of
/// these into `recently_completed` the moment `in_flight.remove(handle)` is
/// about to fire (`is_complete == true`), and the kernel drains it via
/// [`PublishEngine::take_completed`] to flip the `PublishQueueEntry` status
/// from `accepted_locally` to `"ok"` / `"failed"`.
///
/// `accepted` is the relays that landed `PerRelayState::Ok`; `failed` carries
/// the `(relay_url, reason)` pairs from `FailedAfterRetries`. Mixed publishes
/// (at least one Ok + at least one FailedAfterRetries) are reported here with
/// both lists populated — the kernel decides what status string to surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalOutcome {
    pub event_id: String,
    pub accepted: Vec<RelayUrl>,
    pub failed: Vec<(RelayUrl, String)>,
}

pub struct PublishEngine {
    in_flight: HashMap<PublishHandle, InFlight>,
    unavailable_relays: BTreeSet<RelayUrl>,
    pub view: PublishStatusState,
    policy: RetryPolicy,
    outbox: Arc<dyn OutboxResolver>,
    dispatcher: Arc<dyn RelayDispatcher>,
    store: Arc<dyn PublishStore>,
    #[allow(dead_code)]
    signer: Arc<dyn Signer>,
    /// Set when a handle was just removed from `in_flight` (completed or
    /// cancelled) — flush_view consults this so the snapshot's `in_flight`
    /// vector clears the stale row even though nothing in the live map is
    /// marked dirty.
    needs_in_flight_rebuild: bool,
    /// T128: terminal verdicts the engine recorded since the last drain.
    /// Populated in `on_ack` (and any other path that evicts a completed row)
    /// just before `in_flight.remove(handle)`. The kernel drains via
    /// [`PublishEngine::take_completed`] after every engine call to update
    /// the `PublishQueueEntry` projection iOS reads.
    recently_completed: BTreeMap<PublishHandle, TerminalOutcome>,
}

impl PublishEngine {
    pub fn new(
        outbox: Arc<dyn OutboxResolver>,
        dispatcher: Arc<dyn RelayDispatcher>,
        store: Arc<dyn PublishStore>,
        signer: Arc<dyn Signer>,
        policy: RetryPolicy,
    ) -> Self {
        Self {
            in_flight: HashMap::new(),
            unavailable_relays: BTreeSet::new(),
            view: PublishStatusState::new(&Default::default()),
            policy,
            outbox,
            dispatcher,
            store,
            signer,
            needs_in_flight_rebuild: false,
            recently_completed: BTreeMap::new(),
        }
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
                per_relay.insert(url, state);
            }
            let mut pending_retries = BTreeMap::new();
            for (url, due_ms) in record.pending_retries {
                pending_retries.insert(url, due_ms);
            }
            let in_flight = InFlight {
                event: record.event,
                per_relay,
                pending_retries,
                dirty: true,
            };
            self.in_flight.insert(record.handle.clone(), in_flight);
            self.dispatch_pending(&record.handle, now_ms);
        }
        self.flush_view();
        Ok(())
    }

    pub fn start_publish(
        &mut self,
        action: PublishAction,
        now_ms: u64,
    ) -> Result<(), PublishEngineError> {
        match action {
            PublishAction::Publish {
                handle,
                event,
                target,
            } => self.start_publish_inner(handle, event, target, now_ms),
            PublishAction::Cancel { handle } => self.cancel_publish(handle, now_ms),
        }
    }

    fn start_publish_inner(
        &mut self,
        handle: PublishHandle,
        event: SignedEvent,
        target: PublishTarget,
        now_ms: u64,
    ) -> Result<(), PublishEngineError> {
        if self.in_flight.contains_key(&handle) {
            return Err(PublishEngineError::DuplicateHandle(handle));
        }
        let relays = self.outbox.resolve(
            &event.unsigned.pubkey,
            &helpers::collect_p_tags(&event),
            &target,
            event.unsigned.kind,
        );
        if relays.is_empty() {
            self.emit_no_targets(&handle, &event, now_ms);
            return Err(PublishEngineError::NoTargets);
        }
        let mut per_relay = BTreeMap::new();
        for url in &relays {
            per_relay.insert(url.clone(), PerRelayState::Pending);
        }
        self.in_flight.insert(
            handle.clone(),
            InFlight {
                event,
                per_relay,
                pending_retries: BTreeMap::new(),
                dirty: true,
            },
        );
        self.persist(&handle)?;
        self.dispatch_pending(&handle, now_ms);
        self.flush_view();
        Ok(())
    }

    fn cancel_publish(
        &mut self,
        handle: PublishHandle,
        now_ms: u64,
    ) -> Result<(), PublishEngineError> {
        if let Some(mut row) = self.in_flight.remove(&handle) {
            self.needs_in_flight_rebuild = true;
            for state in row.per_relay.values_mut() {
                if !state.is_terminal() {
                    *state = PerRelayState::FailedAfterRetries {
                        reason: "cancelled".to_string(),
                        last_at_ms: now_ms,
                    };
                }
            }
            self.store.delete(&handle)?;
        }
        self.flush_view();
        Ok(())
    }

    /// Drive any per-relay states that are due (Pending → InFlight, or retry
    /// after backoff has elapsed). Called by the actor on its tick.
    pub fn tick(&mut self, now_ms: u64) {
        let deadline_ms = self.policy.inflight_deadline_ms;
        let policy = self.policy;
        let handles: Vec<PublishHandle> = self.in_flight.keys().cloned().collect();
        for handle in &handles {
            if let Some(row) = self.in_flight.get_mut(handle) {
                helpers::sweep_inflight_timeouts(row, now_ms, deadline_ms, policy);
            }
        }
        for handle in &handles {
            self.dispatch_pending(handle, now_ms);
        }
        // Evict handles that became fully terminal during the sweep but were
        // not dispatched (dispatch_due skips terminal states, so on_ack never
        // fires for them). This mirrors the on_ack completion path.
        for handle in handles {
            let Some(in_flight) = self.in_flight.get(&handle) else {
                continue; // already evicted by on_ack during dispatch_pending
            };
            if !helpers::is_complete(in_flight) {
                continue;
            }
            // `in_flight` (bound above by the `let Some(...) else` guard) is
            // still live and points at the same entry — re-fetching with
            // `.get(...).unwrap()` would be a redundant lookup and a D6
            // violation (`unwrap` in a `pub fn`). Reuse the existing borrow.
            helpers::for_each_terminal(in_flight, &handle, &mut self.view, now_ms);
            let outcome = helpers::terminal_outcome_of(in_flight);
            self.recently_completed.insert(handle.clone(), outcome);
            let _ = self.store.delete(&handle);
            self.in_flight.remove(&handle);
            self.needs_in_flight_rebuild = true;
        }
        self.flush_view();
    }

    /// Mark a relay as unavailable for publish delivery. Any event that was
    /// already `InFlight` to that relay moves back to durable `Pending` so a
    /// connection loss never consumes the publish intent.
    pub fn mark_relay_unavailable(
        &mut self,
        relay_url: &str,
        _now_ms: u64,
    ) -> Result<(), PublishEngineError> {
        let relay_url = relay_url.to_string();
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
        self.unavailable_relays.remove(relay_url);
        let handles: Vec<PublishHandle> = self.in_flight.keys().cloned().collect();
        for handle in handles {
            self.dispatch_pending_for_relay(&handle, relay_url, now_ms);
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

    /// Fold a relay ack into the state machine for the given handle.
    pub fn on_ack(&mut self, handle: &PublishHandle, ack: RelayAck, now_ms: u64) {
        let Some(in_flight) = self.in_flight.get_mut(handle) else {
            return;
        };
        let relay_url = helpers::relay_url_of(&ack);
        let Some(state) = in_flight.per_relay.get(&relay_url).cloned() else {
            return;
        };
        let verdict = apply_ack(&state, &ack, self.policy, now_ms);
        helpers::apply_verdict(in_flight, &relay_url, verdict, now_ms);
        if helpers::is_complete(in_flight) {
            helpers::for_each_terminal(in_flight, handle, &mut self.view, now_ms);
            // T128: snapshot the terminal verdict for the kernel's queue-entry
            // projection BEFORE evicting the row. Once `in_flight.remove`
            // runs the per-relay state is gone, and the kernel has no other
            // hook to recover the Ok/Failed map (recent_ok / recent_errors
            // are capped at 32 and not indexed by handle).
            let outcome = helpers::terminal_outcome_of(in_flight);
            self.recently_completed.insert(handle.clone(), outcome);
            if let Err(err) = self.store.delete(handle) {
                self.view.push_failure(RecentFailure {
                    handle: handle.clone(),
                    event_id: in_flight.event.id.clone(),
                    relay_url: "(store)".to_string(),
                    reason: format!("store delete failed: {:?}", err),
                    at_ms: now_ms,
                });
            }
            self.in_flight.remove(handle);
            self.needs_in_flight_rebuild = true;
        } else if let Err(err) = self.persist(handle) {
            // D6: store failure surfaces as a RecentFailure, never panics, never
            // crosses FFI as an exception.
            let event_id = self
                .in_flight
                .get(handle)
                .map(|row| row.event.id.clone())
                .unwrap_or_default();
            self.view.push_failure(RecentFailure {
                handle: handle.clone(),
                event_id,
                relay_url: "(store)".to_string(),
                reason: format!("store upsert failed: {:?}", err),
                at_ms: now_ms,
            });
        }
        self.flush_view();
    }

    /// Snapshot accessor for views / FFI.
    pub fn snapshot(&self) -> &PublishStatusSnapshot {
        &self.view.snapshot
    }

    /// T128: drain every terminal verdict recorded since the last call. The
    /// kernel calls this after every engine entrypoint (`start_publish` /
    /// `on_ack` / `tick` / `resume_from_store`) and applies the verdicts to
    /// its `PublishQueueEntry` projection. Pure drain — the engine retains no
    /// per-publish history after this call (the snapshot's `recent_ok` /
    /// `recent_errors` carry the longer view).
    pub(crate) fn take_completed(&mut self) -> Vec<TerminalOutcome> {
        std::mem::take(&mut self.recently_completed)
            .into_values()
            .collect()
    }

    /// D6 FFI mapping path: convert a `PublishEngineError` into a snapshot
    /// `RecentFailure` row and bump the view rev. The actor / FFI adapter
    /// calls this for any error returned from `start_publish` /
    /// `cancel_publish` / `resume_from_store` before letting the boundary
    /// cross to the platform. Errors never become exceptions; they always
    /// become observable state.
    ///
    /// `event_id` may be empty when the error happens before an event is
    /// associated with a handle.
    pub fn record_engine_error(
        &mut self,
        err: &PublishEngineError,
        handle: &PublishHandle,
        event_id: &str,
        now_ms: u64,
    ) {
        let failure = error_mapping::engine_error_to_failure(err, handle, event_id, now_ms);
        self.view.push_failure(failure);
        self.view.bump_rev();
    }

    /// Engine-owned classification of a raw `RelayAck` (per D7 — capabilities
    /// report; the engine decides policy). The dispatcher MUST NOT call this.
    /// Exposed `pub(crate)` so the FFI bridge (in `crate::ffi::*`) can
    /// inspect a classification without re-deriving the rules; outside callers
    /// must drive the engine through `on_ack` / `tick`.
    ///
    /// `dead_code` allowed because the FFI bridge that calls it lands with
    /// M6 (actor ledger wiring); the in-crate test asserts the routing.
    #[allow(dead_code)]
    pub(crate) fn classify_ack(&self, ack: &RelayAck) -> AckClass {
        classify_ack(ack)
    }

    /// Test/diagnostic accessor — returns the per-relay state map for a
    /// handle, or empty if the publish completed and was evicted.
    pub fn per_relay(&self, handle: &PublishHandle) -> BTreeMap<RelayUrl, PerRelayState> {
        self.in_flight
            .get(handle)
            .map(|row| row.per_relay.clone())
            .unwrap_or_default()
    }

    fn dispatch_pending(&mut self, handle: &PublishHandle, now_ms: u64) {
        self.dispatch_pending_matching(handle, None, now_ms);
    }

    fn dispatch_pending_for_relay(&mut self, handle: &PublishHandle, relay_url: &str, now_ms: u64) {
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

    fn persist(&self, handle: &PublishHandle) -> Result<(), PublishEngineError> {
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
        };
        self.store.upsert(&record).map_err(PublishEngineError::from)
    }

    fn flush_view(&mut self) {
        let mut any_dirty = self.needs_in_flight_rebuild;
        self.needs_in_flight_rebuild = false;
        let mut in_flight_rows = Vec::new();
        for (handle, row) in &mut self.in_flight {
            any_dirty |= row.dirty;
            row.dirty = false;
            in_flight_rows.push(EventPublishStatus {
                handle: handle.clone(),
                event_id: row.event.id.clone(),
                kind: row.event.unsigned.kind,
                created_at: row.event.unsigned.created_at,
                content: row.event.unsigned.content.clone(),
                per_relay: row
                    .per_relay
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            });
        }
        if !any_dirty {
            return;
        }
        self.view.replace_in_flight(in_flight_rows);
        self.view.bump_rev();
    }

    fn emit_no_targets(&mut self, handle: &PublishHandle, event: &SignedEvent, now_ms: u64) {
        self.view.push_failure(RecentFailure {
            handle: handle.clone(),
            event_id: event.id.clone(),
            relay_url: "(none)".to_string(),
            reason: "no relays resolved for publish target".to_string(),
            at_ms: now_ms,
        });
        self.view.bump_rev();
    }
}
