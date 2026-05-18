//! `PublishEngine` — the orchestrator that ties action, state, traits, and
//! view together.
//!
//! Single-threaded by design: the kernel actor (M6 ledger) drives it via
//! `start_publish` / `on_ack` / `tick`. Time is injected (`now_ms`) so the
//! engine is deterministic in tests; the actor passes `Instant::now()` in
//! production.
//!
//! The engine never spawns threads, never touches sockets, and never panics —
//! all failure paths surface through `PublishOutcome` and the snapshot
//! (D6: errors never cross FFI as exceptions).

mod helpers;

pub use helpers::outcome_of;

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::action::{PublishAction, PublishHandle, PublishTarget, RelayUrl};
use super::state::{apply_ack, PerRelayState, RelayAck, RetryPolicy};
use super::traits::{
    OutboxResolver, PublishRecord, PublishStore, PublishStoreError, RelayDispatcher, Signer,
};
use super::view::{
    EventPublishStatus, PublishStatusSnapshot, PublishStatusState, RecentFailure,
};
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

pub struct PublishEngine {
    in_flight: HashMap<PublishHandle, InFlight>,
    pub view: PublishStatusState,
    policy: RetryPolicy,
    outbox: Arc<dyn OutboxResolver>,
    dispatcher: Arc<dyn RelayDispatcher>,
    store: Arc<dyn PublishStore>,
    #[allow(dead_code)]
    signer: Arc<dyn Signer>,
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
            view: PublishStatusState::new(&Default::default()),
            policy,
            outbox,
            dispatcher,
            store,
            signer,
        }
    }

    /// Resume any pending records left by a prior process. Called once at
    /// kernel boot. M3 LMDB will return real rows; the in-memory shim returns
    /// what was previously upserted.
    pub fn resume_from_store(&mut self, now_ms: u64) -> Result<(), PublishEngineError> {
        for record in self.store.load_pending()? {
            let mut per_relay = BTreeMap::new();
            for (url, state) in record.per_relay {
                per_relay.insert(url, state);
            }
            let in_flight = InFlight {
                event: record.event,
                per_relay,
                pending_retries: BTreeMap::new(),
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
            PublishAction::Publish { handle, event, target } => {
                self.start_publish_inner(handle, event, target, now_ms)
            }
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
        let handles: Vec<PublishHandle> = self.in_flight.keys().cloned().collect();
        for handle in handles {
            self.dispatch_pending(&handle, now_ms);
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
        helpers::apply_verdict(in_flight, &relay_url, verdict, now_ms);
        if helpers::is_complete(in_flight) {
            helpers::for_each_terminal(in_flight, handle, &mut self.view, now_ms);
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
        } else {
            let _ = self.persist(handle);
        }
        self.flush_view();
    }

    /// Snapshot accessor for views / FFI.
    pub fn snapshot(&self) -> &PublishStatusSnapshot {
        &self.view.snapshot
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
        let Some(in_flight) = self.in_flight.get_mut(handle) else {
            return;
        };
        let frame = helpers::build_event_frame(&in_flight.event);
        let acks = helpers::dispatch_due(in_flight, now_ms, &*self.dispatcher, &frame);
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
        };
        self.store.upsert(&record).map_err(PublishEngineError::from)
    }

    fn flush_view(&mut self) {
        let mut any_dirty = false;
        let mut in_flight_rows = Vec::new();
        for (handle, row) in &mut self.in_flight {
            any_dirty |= row.dirty;
            row.dirty = false;
            in_flight_rows.push(EventPublishStatus {
                handle: handle.clone(),
                event_id: row.event.id.clone(),
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
