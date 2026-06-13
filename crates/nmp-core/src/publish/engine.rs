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

mod dispatch;
mod error_mapping;
mod helpers;
#[cfg(test)]
mod auth_park_tests;
#[cfg(test)]
mod tests;
mod types;
mod view_ops;

pub use error_mapping::{engine_error_to_failure, ENGINE_FAILURE_RELAY_URL};
pub use helpers::outcome_of;
pub use types::{LastTerminal, TerminalOutcome};
// Re-exported for `engine::helpers` which references both type names via
// `super::{InFlight, TerminalOutcome}`. `InFlight` stays crate-private (it is
// an internal engine detail, not part of any public surface) while
// `TerminalOutcome` rides out through the engine's public `take_completed`.
pub(super) use types::InFlight;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::action::{PublishAction, PublishHandle, PublishTarget, RelayUrl};
use super::state::{apply_ack, classify_ack, AckClass, PerRelayState, RelayAck, RetryPolicy};
use super::traits::{
    OutboxResolver, PublishStore, PublishStoreError, RelayDispatcher, RelaySelectionReason, Signer,
};
use super::view::{PublishStatusSnapshot, PublishStatusState, RecentFailure};
use crate::substrate::{empty_blocked_relay_lookup, BlockedRelayLookup, SignedEvent};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PublishEngineError {
    DuplicateHandle(PublishHandle),
    NoTargets,
    Store(PublishStoreError),
    /// The engine was handed a `PublishAction` variant it does not service —
    /// the actor-signed variants (`PublishRaw` → `ActorCommand::PublishRawEvent`,
    /// `PublishProfile` → `ActorCommand::PublishProfile`), which the actor signs
    /// and publishes, not this engine. The `ActionRegistry` executor routes
    /// those to the actor directly, so reaching `start_publish` with one is a
    /// wiring bug. Surfaced as an `Err` (never an `unreachable!`) so D6 holds —
    /// the invariant violation becomes snapshot-visible state, never a panic.
    UnsupportedAction(&'static str),
}

impl std::fmt::Display for PublishEngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateHandle(h) => write!(f, "duplicate publish handle: {:?}", h),
            Self::NoTargets => write!(f, "no relay targets for publish"),
            Self::Store(e) => write!(f, "publish store error: {e}"),
            Self::UnsupportedAction(name) => write!(f, "unsupported action: {name}"),
        }
    }
}

impl std::error::Error for PublishEngineError {}

impl From<PublishStoreError> for PublishEngineError {
    fn from(err: PublishStoreError) -> Self {
        Self::Store(err)
    }
}

pub struct PublishEngine {
    in_flight: HashMap<PublishHandle, InFlight>,
    unavailable_relays: BTreeSet<RelayUrl>,
    pub view: PublishStatusState,
    policy: RetryPolicy,
    outbox: Arc<dyn OutboxResolver>,
    /// Per-account blocked-relay lookup (kind:10006). Consulted at resolve
    /// time so the outbox resolver can exclude relays the author told us to
    /// never publish to. Default is [`empty_blocked_relay_lookup`] (no
    /// blocks); production composition installs the router-side
    /// `InMemoryBlockedRelayCache` via [`PublishEngine::set_blocked_relay_lookup`].
    blocked_relays: Arc<dyn BlockedRelayLookup>,
    dispatcher: Arc<dyn RelayDispatcher>,
    store: Arc<dyn PublishStore>,
    #[allow(dead_code)]
    signer: Arc<dyn Signer>,
    /// Set when a handle was just removed from `in_flight` (completed or
    /// cancelled) — `flush_view` consults this so the snapshot's `in_flight`
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
    #[must_use]
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
            view: PublishStatusState::new(&super::view::PublishStatusSpec::default()),
            policy,
            outbox,
            blocked_relays: empty_blocked_relay_lookup(),
            dispatcher,
            store,
            signer,
            needs_in_flight_rebuild: false,
            recently_completed: BTreeMap::new(),
            pending_terminals: Vec::new(),
        }
    }

    /// Swap the engine's `OutboxResolver` in-place.
    ///
    /// Spec §271 (2026-05-25): the kernel constructs `PublishEngine` with
    /// the in-crate `NoopOutboxResolver` default, then production
    /// composition (`nmp-defaults::register_defaults` →
    /// `Kernel::set_publish_resolver`) swaps in
    /// `nmp_router::Nip65OutboxResolver`. MUST be called BEFORE any publish
    /// reaches `start_publish` — swapping mid-publish would leave the
    /// in-flight resolver decisions inconsistent with subsequent retries
    /// (the engine's `dispatch_due` path re-asks the current resolver on
    /// every tick).
    pub fn set_outbox(&mut self, outbox: Arc<dyn OutboxResolver>) {
        self.outbox = outbox;
    }

    /// Swap the engine's [`BlockedRelayLookup`] in-place. Production
    /// composition (`nmp-defaults::register_defaults` → kernel wiring)
    /// installs the router-side `InMemoryBlockedRelayCache` so the outbox
    /// resolver excludes the active account's kind:10006 blocked relays. The
    /// default is [`empty_blocked_relay_lookup`] (no blocks — a kernel built
    /// without the router-side cache behaves exactly as before this seam
    /// existed). MUST be installed before any publish reaches `start_publish`
    /// for the block to take effect on that publish.
    pub fn set_blocked_relay_lookup(&mut self, lookup: Arc<dyn BlockedRelayLookup>) {
        self.blocked_relays = lookup;
    }

    /// Drive a `PublishAction` into the engine.
    ///
    /// `correlation_id_override` is the action `correlation_id` to report in
    /// `action_results` when it differs from the publish handle — set for
    /// the `PublishRaw` dispatch path (the actor signs the event, so the host
    /// received a registry-minted id, not the event id). `None` for every
    /// other caller: the terminal verdict then reports the handle, preserving
    /// the prior behaviour. Only the `Publish` variant carries the override
    /// into an `InFlight` row; `Cancel` already reports `handle` as the
    /// `correlation_id` (which is what the host got back from dispatch).
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
            // `PublishProfile` is signed-and-published by the actor's
            // `ActorCommand::PublishProfile` handler; the engine only services
            // pre-signed `Publish` (and `Cancel`). The `ActionRegistry`
            // executor routes `PublishProfile` to `ActorCommand::PublishProfile`,
            // never to this engine. Reaching here is a wiring bug — D6 forbids
            // surfacing it as a panic / `unreachable!`, so it is returned as an
            // `Err` the caller maps to snapshot-visible state.
            PublishAction::PublishProfile { .. } => Err(PublishEngineError::UnsupportedAction(
                "PublishProfile is published via ActorCommand::PublishProfile, not the publish engine",
            )),
            // `PublishRaw` is signed-and-published by the actor's
            // `ActorCommand::PublishRawEvent` handler (which delegates to the
            // existing `publish_unsigned_event{,_to_relays}` helpers) — same
            // rationale as `PublishProfile`. Reaching here is a wiring bug
            // returned as an `Err`, never a panic (D6).
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
        // Resolve the author's kind:10006 blocked-relay set once and pass it
        // into the resolver so every selected relay (write set, fallback,
        // discovery indexers, recipient inboxes, and even explicit targets)
        // is filtered against it. Privacy fix: before this the outbox
        // resolver had no blocked set and leaked publishes to relays the
        // author explicitly blocked.
        let blocked = self.blocked_relays.blocked_relays(&event.unsigned.pubkey);
        let resolved = self.outbox.resolve(
            &event.unsigned.pubkey,
            &helpers::collect_p_tags(&event),
            &target,
            event.unsigned.kind,
            &blocked,
        );
        // Deduplicate by canonical URL. When the same canonical URL appears
        // with multiple distinct reasons (e.g. NIP-65 write relay AND a
        // discovery indexer for kind:0), collect them into a `Vec` so the
        // projection can render both rationales instead of an arbitrary one.
        // The publish engine remains the single owner of canonicalization
        // (the resolver returns whatever URL form the caller stored in
        // kind:10002).
        let mut relay_map: BTreeMap<RelayUrl, Vec<RelaySelectionReason>> = BTreeMap::new();
        for r in resolved {
            let canonical = helpers::canonical_relay_identity(&r.url);
            let bucket = relay_map.entry(canonical).or_default();
            if !bucket.contains(&r.reason) {
                bucket.push(r.reason);
            }
        }
        if relay_map.is_empty() {
            self.emit_no_targets(&handle, &event, correlation_id_override.as_deref(), now_ms);
            return Err(PublishEngineError::NoTargets);
        }
        let per_relay: BTreeMap<RelayUrl, PerRelayState> = relay_map
            .keys()
            .map(|url| (url.clone(), PerRelayState::Pending))
            .collect();
        let relay_reasons = relay_map;
        self.in_flight.insert(
            handle.clone(),
            InFlight {
                event,
                per_relay,
                relay_reasons,
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
            result_json: None,
        });
        self.flush_view();
        Ok(())
    }

    /// Drive any per-relay states that are due (Pending → `InFlight`, or retry
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
        let park_awaiting_auth = helpers::apply_verdict(in_flight, &relay_url, verdict, now_ms);
        if park_awaiting_auth {
            // The relay refused the EVENT pending NIP-42 auth. Route it through
            // the single availability gate: `mark_relay_unavailable` demotes the
            // InFlight send back to durable `Pending`, drops any scheduled
            // retry, persists, and parks the relay in `unavailable_relays` so no
            // retry tick re-dispatches it. The publish stays in-flight; it
            // re-dispatches event-driven when the kernel calls
            // `mark_relay_available` on the `RelayAuthState::Authenticated`
            // transition (no budget spent, no sleep/poll — D8). Borrow of
            // `in_flight` ends above, so the `&mut self` call is sound.
            if let Err(err) = self.mark_relay_unavailable(&relay_url, now_ms) {
                self.record_engine_error(&err, handle, "", now_ms);
            }
            self.flush_view();
            return;
        }
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
                    reason: format!("store delete failed: {err:?}"),
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
                reason: format!("store upsert failed: {err:?}"),
                at_ms: now_ms,
            });
        }
        self.flush_view();
    }

    /// Snapshot accessor for views / FFI.
    #[must_use]
    pub fn snapshot(&self) -> &PublishStatusSnapshot {
        &self.view.snapshot
    }

    pub(crate) fn has_active_relay(&self, relay_url: &str) -> bool {
        let key = helpers::canonical_relay_identity(relay_url);
        self.in_flight
            .values()
            .any(|row| row.per_relay.contains_key(&key) || row.pending_retries.contains_key(&key))
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
    #[must_use]
    pub fn per_relay(&self, handle: &PublishHandle) -> BTreeMap<RelayUrl, PerRelayState> {
        self.in_flight
            .get(handle)
            .map(|row| row.per_relay.clone())
            .unwrap_or_default()
    }
}
