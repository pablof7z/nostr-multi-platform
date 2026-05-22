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
    /// The engine was handed a `PublishAction` variant it does not service —
    /// currently only `PublishAction::PublishNote`, which is signed and
    /// published by the actor's `ActorCommand::PublishNote` handler, not by
    /// this engine. The `ActionRegistry` executor routes `PublishNote` to the
    /// actor directly, so reaching `start_publish` with one is a wiring bug.
    /// Surfaced as an `Err` (never an `unreachable!`) so D6 holds — the
    /// invariant violation becomes snapshot-visible state, never a panic.
    UnsupportedAction(&'static str),
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
    /// Optional action correlation_id to report in `LastTerminal` instead of
    /// the publish `handle` (== event id). Set when the publish originates
    /// from `nmp_app_dispatch_action`'s `PublishAction::PublishNote` path: the
    /// actor signs the event, so its `id` is not known at dispatch time and
    /// the host received a registry-minted correlation_id that differs from
    /// the event id. The terminal sites (`on_ack`, `tick`) report this id so
    /// the host spinner can be cleared. `None` for every other publish path
    /// (pre-signed `Publish`, `react`, `follow`, …) — the terminal verdict
    /// then uses the `handle`, preserving prior behaviour.
    pub correlation_id_override: Option<String>,
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

/// Direction review #29: one terminal action result the engine records into
/// `pending_terminals` so the kernel can drain it into the `action_results`
/// snapshot projection. The host reads `action_results` to clear a per-action
/// spinner — each tick surfaces every action that settled, not just the most
/// recent.
///
/// `correlation_id` is the `PublishHandle` (== `event_id` for publish actions).
/// `status` uses the engine's internal vocabulary `"ok" | "failed" |
/// "cancelled"`; the kernel translates `"ok" → "published"` at the projection
/// serialization site. `error` is `None` for success, otherwise a single
/// human-readable string (the per-relay failure reasons joined with `; `).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LastTerminal {
    pub correlation_id: PublishHandle,
    pub status: &'static str,
    pub error: Option<String>,
}

impl LastTerminal {
    /// Build a `LastTerminal` from a settled `TerminalOutcome`. Mirrors the
    /// kernel's `classify_terminal_outcome` status rule: any accepted relay →
    /// `"ok"`, otherwise `"failed"`.
    ///
    /// `correlation_id_override` is the action correlation_id the host received
    /// from `nmp_app_dispatch_action` when it differs from the publish handle
    /// (the `PublishNote` path — the actor signs the event, so the host got a
    /// registry-minted id, not the event id). When `Some`, the returned
    /// `correlation_id` is that override; when `None`, it falls back to the
    /// `handle` (the pre-existing behaviour for every other publish path).
    fn from_outcome(
        handle: &PublishHandle,
        correlation_id_override: Option<&str>,
        outcome: &TerminalOutcome,
    ) -> Self {
        let correlation_id = correlation_id_override
            .map(str::to_string)
            .unwrap_or_else(|| handle.clone());
        if outcome.accepted.is_empty() {
            let error = if outcome.failed.is_empty() {
                Some("publish failed: no relays settled".to_string())
            } else {
                Some(
                    outcome
                        .failed
                        .iter()
                        .map(|(url, reason)| format!("{}: {}", url, reason))
                        .collect::<Vec<_>>()
                        .join("; "),
                )
            };
            Self {
                correlation_id,
                status: "failed",
                error,
            }
        } else {
            Self {
                correlation_id,
                status: "ok",
                error: None,
            }
        }
    }
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
    /// the `PublishQueueEntry` projection the shell reads.
    recently_completed: BTreeMap<PublishHandle, TerminalOutcome>,
    /// Direction review #29: every terminal action result that settled since
    /// the last drain. This Vec *accumulates* — so when two actions reach a
    /// terminal state between two snapshot emits, both are retained. The
    /// kernel drains it via [`Self::take_pending_terminals`] into the
    /// `action_results` snapshot projection so the host can resolve every
    /// spinner, not just the most recent.
    pending_terminals: Vec<LastTerminal>,
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
            pending_terminals: Vec::new(),
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
                per_relay.insert(helpers::canonical_relay_identity(&url), state);
            }
            let mut pending_retries = BTreeMap::new();
            for (url, due_ms) in record.pending_retries {
                pending_retries.insert(helpers::canonical_relay_identity(&url), due_ms);
            }
            let in_flight = InFlight {
                event: record.event,
                per_relay,
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

    /// Drive a `PublishAction` into the engine.
    ///
    /// `correlation_id_override` is the action correlation_id to report in
    /// `action_results` when it differs from the publish handle — set for
    /// the `PublishNote` dispatch path (the actor signs the event, so the host
    /// received a registry-minted id, not the event id). `None` for every
    /// other caller: the terminal verdict then reports the handle, preserving
    /// the prior behaviour. Only the `Publish` variant carries the override
    /// into an `InFlight` row; `Cancel` already reports `handle` as the
    /// correlation_id (which is what the host got back from dispatch).
    pub fn start_publish(
        &mut self,
        action: PublishAction,
        now_ms: u64,
        correlation_id_override: Option<String>,
    ) -> Result<(), PublishEngineError> {
        match action {
            PublishAction::Publish {
                handle,
                event,
                target,
            } => self.start_publish_inner(handle, event, target, correlation_id_override, now_ms),
            PublishAction::Cancel { handle } => self.cancel_publish(handle, now_ms),
            // `PublishNote` is signed-and-published by the actor's
            // `ActorCommand::PublishNote` handler; the engine only services
            // pre-signed `Publish` (and `Cancel`). The `ActionRegistry`
            // executor routes `PublishNote` to `ActorCommand::PublishNote`,
            // never to this engine. Reaching here is a wiring bug — D6
            // forbids surfacing it as a panic / `unreachable!`, so it is
            // returned as an `Err` the caller maps to snapshot-visible state.
            PublishAction::PublishNote { .. } => Err(PublishEngineError::UnsupportedAction(
                "PublishNote is published via ActorCommand::PublishNote, not the publish engine",
            )),
            // `PublishProfile` is signed-and-published by the actor's
            // `ActorCommand::PublishProfile` handler — same rationale as
            // `PublishNote`: the engine only services pre-signed `Publish`
            // (and `Cancel`). Reaching here is a wiring bug returned as an
            // `Err`, never a panic (D6).
            PublishAction::PublishProfile { .. } => Err(PublishEngineError::UnsupportedAction(
                "PublishProfile is published via ActorCommand::PublishProfile, not the publish engine",
            )),
            // `PublishRaw` is signed-and-published by the actor's
            // `ActorCommand::PublishRawEvent` handler (which delegates to the
            // existing `publish_unsigned_event{,_to_relays}` helpers) — same
            // rationale as `PublishNote`/`PublishProfile`. Reaching here is a
            // wiring bug returned as an `Err`, never a panic (D6).
            PublishAction::PublishRaw { .. } => Err(PublishEngineError::UnsupportedAction(
                "PublishRaw is published via ActorCommand::PublishRawEvent, not the publish engine",
            )),
        }
    }

    fn start_publish_inner(
        &mut self,
        handle: PublishHandle,
        event: SignedEvent,
        target: PublishTarget,
        correlation_id_override: Option<String>,
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
        let relays = helpers::canonicalize_relay_set(relays);
        if relays.is_empty() {
            self.emit_no_targets(&handle, &event, correlation_id_override.as_deref(), now_ms);
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
                correlation_id_override,
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
        // Direction review #24: cancellation is a terminal action result, but
        // it never flows through `recently_completed` (the kernel surfaces
        // "cancelled" separately via `set_publish_entry_terminal`). Record it
        // here directly so `action_results` clears the host spinner — even a
        // cancel for an unknown / already-settled handle is a terminal verdict
        // the host asked for.
        self.record_terminal(LastTerminal {
            correlation_id: handle,
            status: "cancelled",
            error: None,
        });
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
            helpers::for_each_terminal(in_flight, &handle, &mut self.view, now_ms);
            let outcome = helpers::terminal_outcome_of(in_flight);
            // Build the verdict into a local before `record_terminal` (a
            // `&mut self` method) so it does not reborrow `*self` while the
            // `in_flight` immutable borrow above is still live.
            let terminal = LastTerminal::from_outcome(
                &handle,
                in_flight.correlation_id_override.as_deref(),
                &outcome,
            );
            self.record_terminal(terminal);
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
            // Build the terminal verdict into a local AND read `event_id` off
            // `in_flight` before calling `record_terminal` — that method takes
            // `&mut self`, so reborrowing `*self` while the `in_flight` borrow
            // is still live (it is used in the store-delete failure branch
            // below) would be an aliasing violation.
            let terminal = LastTerminal::from_outcome(
                handle,
                in_flight.correlation_id_override.as_deref(),
                &outcome,
            );
            let event_id = in_flight.event.id.clone();
            self.record_terminal(terminal);
            self.recently_completed.insert(handle.clone(), outcome);
            if let Err(err) = self.store.delete(handle) {
                self.view.push_failure(RecentFailure {
                    handle: handle.clone(),
                    event_id,
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

    fn emit_no_targets(
        &mut self,
        handle: &PublishHandle,
        event: &SignedEvent,
        correlation_id_override: Option<&str>,
        now_ms: u64,
    ) {
        self.view.push_failure(RecentFailure {
            handle: handle.clone(),
            event_id: event.id.clone(),
            relay_url: "(none)".to_string(),
            reason: "no relays resolved for publish target".to_string(),
            at_ms: now_ms,
        });
        // Direction review #24: NoTargets is a terminal "failed" outcome — the
        // publish never gets queued and `start_publish` returns Err(NoTargets),
        // so it never reaches the `recently_completed` / `on_ack` paths.
        // Record it here so `action_results` reports the failure and the
        // host clears its spinner instead of waiting on an op that never ran.
        //
        // Report the dispatch correlation_id when one was supplied (the
        // `PublishNote` path), otherwise the handle — same fallback rule as
        // `LastTerminal::from_outcome`.
        self.record_terminal(LastTerminal {
            correlation_id: correlation_id_override
                .map(str::to_string)
                .unwrap_or_else(|| handle.clone()),
            status: "failed",
            error: Some("no relays resolved for publish target".to_string()),
        });
        self.view.bump_rev();
    }

    /// Direction review #29: record one terminal action verdict by appending
    /// to `pending_terminals` (the per-tick drain that fixes the spinner-hang
    /// bug — two settlements in one tick both survive). Every site that
    /// produces a terminal verdict routes through here.
    fn record_terminal(&mut self, terminal: LastTerminal) {
        self.pending_terminals.push(terminal);
    }

    /// Record a terminal `"failed"` verdict for a dispatched action that never
    /// reached the publish engine's in-flight set — the event was never signed,
    /// so there is no `PublishHandle` and no `TerminalOutcome`.
    ///
    /// This closes a broken-promise gap: a host that dispatched a
    /// `PublishNote` / `PublishProfile` through `nmp_app_dispatch_action`
    /// received a registry-minted `correlation_id` and is waiting to see its
    /// outcome in the `action_results` snapshot projection. When the *sign*
    /// step fails (no active account, a malformed reply id, a local-key sign
    /// error, or a remote-signer timeout / rejection) the publish never
    /// happens — without this entry the host's spinner keyed on that
    /// `correlation_id` would hang forever.
    ///
    /// Unlike [`Self::record_engine_error`] this does **not** push a
    /// `RecentFailure` row: no event/handle exists to anchor one, and the
    /// caller already surfaces a `set_last_error_toast`. This records *only*
    /// the `action_results` terminal so the dispatched action's promise is
    /// honoured.
    pub(crate) fn record_action_terminal_failure(
        &mut self,
        correlation_id: String,
        error: String,
    ) {
        self.record_terminal(LastTerminal {
            correlation_id,
            status: "failed",
            error: Some(error),
        });
    }

    /// Record a terminal `"ok"` verdict for a dispatched action that completed
    /// **without** going through the publish-engine in-flight set — i.e. the
    /// outcome is observed off-band, not via a relay OK on a signed event.
    ///
    /// The motivating consumer is NIP-47 NWC `pay_invoice`: the action's
    /// terminal outcome is the **wallet's** kind:23195 response carrying a
    /// `preimage`. That response never reaches the publish engine (the
    /// kind:23194 request itself settles separately as a normal publish; the
    /// *payment* outcome lives in the NWC response channel), so a host that
    /// dispatched the payment through `nmp_app_dispatch_action` would
    /// otherwise have no `action_results` entry to drain its spinner — the
    /// same broken-promise gap [`Self::record_action_terminal_failure`] closes
    /// for sign-step failures.
    ///
    /// Mirrors [`Self::record_action_terminal_failure`]: pushes a single
    /// `LastTerminal { status: "ok", error: None }` onto `pending_terminals`
    /// for the next snapshot drain. No `RecentFailure` row is written (success
    /// paths don't anchor failure rows); the caller is responsible for any
    /// projection-level state (e.g. wallet balance refresh) it needs.
    // `#[allow(dead_code)]`: the sole live caller today is
    // `Kernel::record_action_success` (publish_cmd.rs), which is in turn only
    // invoked by `handle_nwc_text` in the wallet runtime — itself gated behind
    // the `wallet` Cargo feature. A plain `cargo check -p nmp-core` (default
    // features) sees no consumer of either method, and the per-crate dead-code
    // lint fires; the cross-feature truth (every `--features wallet` build
    // wires both) is invisible to rustc here.
    #[allow(dead_code)]
    pub(crate) fn record_action_terminal_success(&mut self, correlation_id: String) {
        self.record_terminal(LastTerminal {
            correlation_id,
            status: "ok",
            error: None,
        });
    }

    /// Direction review #29: drain every terminal verdict recorded since the
    /// last call. The kernel calls this from the snapshot path
    /// (`make_update` → `take_action_results_projection`) so each tick surfaces
    /// every action that settled. Pure drain: after this call the engine
    /// retains no per-tick terminal history.
    pub(crate) fn take_pending_terminals(&mut self) -> Vec<LastTerminal> {
        std::mem::take(&mut self.pending_terminals)
    }
}
