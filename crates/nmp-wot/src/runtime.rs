use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use nmp_core::slots::ActiveAccountSlot;
use nmp_core::substrate::{
    register_observer_projection, LiveEventTapRegistrar, HostCapabilities, KernelEvent,
    SnapshotProjectionRegistrar,
};
use nmp_core::KernelEventObserver;
use nmp_planner::LogicalInterest;
use serde::Serialize;

use crate::interest::{
    active_follow_graph_identity, follow_graph_interest, is_hex_pubkey, KIND_CONTACT_LIST,
};
use crate::score::{TrustDecision, WotGraph, WotGraphStats};

/// Register the WOT graph observer and bootstrap controller.
///
/// Returns the installed runtime so app crates can read trust scores from the
/// exact graph maintained by the observer. Existing composition roots may
/// ignore the handle when they only need the bootstrap side effects.
pub fn register_runtime(
    app: &(impl HostCapabilities + LiveEventTapRegistrar + SnapshotProjectionRegistrar),
) -> Option<Arc<WotBootstrapRuntime>> {
    let runtime = Arc::new(WotBootstrapRuntime::new(
        // Pubkey-only identity (Finding C): the WOT bootstrap needs the active
        // account's pubkey, never its secret key — read the slot the kernel
        // populates for every backend so bunker accounts bootstrap too.
        app.active_pubkey(),
        app.actor_sender(),
    ));
    // register_observer_projection handles the observer-slot-poisoned guard (#1724 criterion 3).
    let projection_runtime = Arc::clone(&runtime);
    let observer_id = register_observer_projection(
        app,
        Arc::clone(&runtime) as Arc<dyn KernelEventObserver>,
        "nmp.wot.bootstrap",
        move || projection_runtime.snapshot_typed(),
    )?;
    if let Some(previous) = app.swap_singleton_event_observer(Some(observer_id)) {
        app.unregister_event_observer(previous);
    }
    Some(runtime)
}

/// Runtime controller that watches kind:3/kind:10000 arrivals and emits the
/// active account's large replaceable-kind bootstrap interest.
pub struct WotBootstrapRuntime {
    /// Pubkey-only identity slot (Finding C): the active account's hex pubkey,
    /// populated by the kernel for every backend including bunker. The runtime
    /// only ever needs identity, never secret key material.
    active_pubkey: ActiveAccountSlot,
    tx: nmp_core::CommandSender,
    state: Mutex<WotRuntimeState>,
}

#[derive(Default)]
struct WotRuntimeState {
    active_pubkey: Option<String>,
    active_follows: BTreeSet<String>,
    bootstrap_pushed: bool,
    graph: WotGraph,
}

/// Small diagnostic noun the WOT bootstrap runtime projects onto the kernel
/// snapshot under `nmp.wot.bootstrap`. The serde JSON of this struct is the
/// authoritative `register_snapshot_projection` shape; the typed FlatBuffers
/// sidecar (`crate::wire::typed_fb`) mirrors it field-for-field (ADR-0037).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WotBootstrapSnapshot {
    /// Active account hex pubkey, or `None` when no account is selected.
    pub active_pubkey: Option<String>,
    /// Number of follows in the active account's contact list.
    pub active_follow_count: usize,
    /// Whether the large replaceable-kind bootstrap interest has been pushed.
    pub bootstrap_requested: bool,
    /// Distinct follow-edge authors in the in-memory WOT graph.
    pub graph_follow_authors: usize,
    /// Distinct mute-edge authors in the in-memory WOT graph.
    pub graph_mute_authors: usize,
}

impl WotBootstrapRuntime {
    /// Construct a runtime around the active-pubkey slot and actor command
    /// sender. The slot carries the active account's hex pubkey only — never
    /// secret key material — so the runtime activates for bunker accounts.
    #[must_use]
    pub fn new(active_pubkey: ActiveAccountSlot, tx: nmp_core::CommandSender) -> Self {
        Self {
            active_pubkey,
            tx,
            state: Mutex::new(WotRuntimeState::default()),
        }
    }

    /// Tick account-change cleanup and build the diagnostic snapshot shape.
    ///
    /// Single source of truth for both projection closures: the generic JSON
    /// projection ([`Self::snapshot_json`]) and the typed FlatBuffers sidecar
    /// ([`Self::snapshot_typed`]) both derive their payload from this struct.
    /// Returns `None` only when the state lock is poisoned — never on an empty
    /// graph or absent account (those round-trip as zero counts / `None`
    /// pubkey), so the two projections emit in lock-step.
    #[must_use]
    pub fn current_snapshot(&self) -> Option<WotBootstrapSnapshot> {
        let active = self.active_pubkey();
        let mut state = self.state.lock().ok()?;
        if state.active_pubkey != active {
            if state.bootstrap_pushed {
                self.withdraw_bootstrap();
            }
            state.active_pubkey = active.clone();
            state.active_follows.clear();
            state.bootstrap_pushed = false;
        }
        Some(WotBootstrapSnapshot {
            active_pubkey: state.active_pubkey.clone(),
            active_follow_count: state.active_follows.len(),
            bootstrap_requested: state.bootstrap_pushed,
            graph_follow_authors: state.graph.follow_author_count(),
            graph_mute_authors: state.graph.mute_author_count(),
        })
    }

    /// Tick account-change cleanup and expose a small diagnostic snapshot as
    /// the authoritative serde JSON value (the generic `Value` projection).
    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        match self.current_snapshot() {
            Some(snapshot) => serde_json::to_value(snapshot).unwrap_or(serde_json::Value::Null),
            None => serde_json::Value::Null,
        }
    }

    /// Build the typed FlatBuffers sidecar entry for the `nmp.wot.bootstrap`
    /// projection, or `None` when the state lock is poisoned (matching the
    /// generic projection's only `Null` condition, so the typed sidecar is
    /// emitted whenever — and only whenever — the JSON projection is non-Null).
    #[must_use]
    pub fn snapshot_typed(&self) -> Option<nmp_core::TypedProjectionData> {
        let snapshot = self.current_snapshot()?;
        Some(crate::wire::typed_fb::typed_projection(&snapshot))
    }

    /// Score one candidate with the default NMP trust policy.
    #[must_use]
    pub fn score(&self, viewer: &str, candidate: &str) -> Option<TrustDecision> {
        self.state
            .lock()
            .ok()
            .map(|state| state.graph.score(viewer, candidate))
    }

    /// Score one candidate with a caller-supplied minimum-score floor.
    #[must_use]
    pub fn score_with_minimum_score(
        &self,
        viewer: &str,
        candidate: &str,
        minimum_score: i32,
    ) -> Option<TrustDecision> {
        self.state.lock().ok().map(|state| {
            state
                .graph
                .score_with_minimum_score(viewer, candidate, minimum_score)
        })
    }

    /// Score multiple candidates with the default NMP trust policy.
    #[must_use]
    pub fn batch_score(&self, viewer: &str, candidates: &[String]) -> Option<Vec<TrustDecision>> {
        self.state.lock().ok().map(|state| {
            candidates
                .iter()
                .map(|candidate| state.graph.score(viewer, candidate))
                .collect()
        })
    }

    /// Score multiple candidates with a caller-supplied minimum-score floor.
    #[must_use]
    pub fn batch_score_with_minimum_score(
        &self,
        viewer: &str,
        candidates: &[String],
        minimum_score: i32,
    ) -> Option<Vec<TrustDecision>> {
        self.state.lock().ok().map(|state| {
            candidates
                .iter()
                .map(|candidate| {
                    state
                        .graph
                        .score_with_minimum_score(viewer, candidate, minimum_score)
                })
                .collect()
        })
    }

    /// Pubkeys followed by `viewer` who also follow `candidate`.
    #[must_use]
    pub fn mutual_follows(&self, viewer: &str, candidate: &str) -> Option<Vec<String>> {
        self.state
            .lock()
            .ok()
            .map(|state| state.graph.mutual_follows(viewer, candidate))
    }

    /// Current graph size counters.
    #[must_use]
    pub fn graph_stats(&self) -> Option<WotGraphStats> {
        self.state.lock().ok().map(|state| state.graph.stats())
    }

    /// Return second-degree candidates ranked by mutual-follow count.
    ///
    /// Delegates to [`WotGraph::ranked_second_degree_candidates`]; see its
    /// documentation for the exact ranking semantics. Returns `None` on a
    /// poisoned lock (not on an empty graph or absent account).
    #[must_use]
    pub fn ranked_second_degree_candidates(
        &self,
        viewer: &str,
        limit: usize,
    ) -> Option<Vec<(String, usize)>> {
        self.state
            .lock()
            .ok()
            .map(|state| state.graph.ranked_second_degree_candidates(viewer, limit))
    }

    fn active_pubkey(&self) -> Option<String> {
        // Identity straight from the pubkey slot — already a hex string, so no
        // keypair derivation. `None` on a poisoned lock or no signed-in account.
        self.active_pubkey.lock().ok().and_then(|slot| slot.clone())
    }

    fn reconcile_active_follows(&self, author: &str, follows: BTreeSet<String>) {
        let mut next_interest = None;
        let mut withdraw = false;
        {
            let mut state = match self.state.lock() {
                Ok(state) => state,
                Err(_) => return,
            };
            if state.active_pubkey.as_deref() != Some(author) {
                if state.bootstrap_pushed {
                    withdraw = true;
                }
                state.active_pubkey = Some(author.to_string());
                state.active_follows.clear();
                state.bootstrap_pushed = false;
            }
            if state.active_follows == follows && state.bootstrap_pushed {
                return;
            }
            if follows.is_empty() {
                withdraw = state.bootstrap_pushed || withdraw;
                state.active_follows.clear();
                state.bootstrap_pushed = false;
            } else {
                withdraw |= state.bootstrap_pushed;
                next_interest = follow_graph_interest(follows.iter().cloned());
                state.active_follows = follows;
                state.bootstrap_pushed = next_interest.is_some();
            }
        }

        if withdraw {
            self.withdraw_bootstrap();
        }
        if let Some(interest) = next_interest {
            self.push_bootstrap(interest);
        }
    }

    fn push_bootstrap(&self, interest: LogicalInterest) {
        self.tx
            .ensure_interest(active_follow_graph_identity(), interest);
    }

    fn withdraw_bootstrap(&self) {
        self.tx.drop_interest_owner(active_follow_graph_identity());
    }
}

impl KernelEventObserver for WotBootstrapRuntime {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if let Ok(mut state) = self.state.lock() {
            state
                .graph
                .ingest_event(&event.author, event.kind, event.tags.as_slice());
        }

        if event.kind != KIND_CONTACT_LIST {
            return;
        }
        let active = self.active_pubkey();
        if active.as_deref() != Some(event.author.as_str()) {
            return;
        }
        let follows = event
            .tags
            .iter()
            .filter_map(|tag| {
                if tag.first().is_some_and(|name| name == "p") {
                    tag.get(1).filter(|value| is_hex_pubkey(value)).cloned()
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>();
        self.reconcile_active_follows(&event.author, follows);
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
