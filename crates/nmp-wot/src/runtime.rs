use std::collections::BTreeSet;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use nmp_core::planner::LogicalInterest;
use nmp_core::slots::ActiveLocalKeysSlot;
use nmp_core::substrate::KernelEvent;
use nmp_core::{ActorCommand, KernelEventObserver, KernelEventObserverId};
use nmp_ffi::NmpApp;
use serde::Serialize;

use crate::interest::{
    active_follow_graph_interest_id, follow_graph_interest, is_hex_pubkey, KIND_CONTACT_LIST,
};
use crate::score::WotGraph;

/// Register the WOT graph observer and bootstrap controller.
pub fn register_runtime(app: &NmpApp) {
    let runtime = Arc::new(WotBootstrapRuntime::new(
        app.active_local_keys(),
        app.actor_sender(),
    ));
    let observer_id =
        app.register_event_observer(Arc::clone(&runtime) as Arc<dyn KernelEventObserver>);
    if observer_id == KernelEventObserverId(0) {
        return;
    }
    if let Some(previous) = app.swap_singleton_event_observer(Some(observer_id)) {
        app.unregister_event_observer(previous);
    }
    app.register_snapshot_projection("nmp.wot.bootstrap", move || runtime.snapshot_json());
}

/// Runtime controller that watches kind:3/kind:10000 arrivals and emits the
/// active account's large replaceable-kind bootstrap interest.
pub struct WotBootstrapRuntime {
    local_keys: ActiveLocalKeysSlot,
    tx: Sender<ActorCommand>,
    state: Mutex<WotRuntimeState>,
}

#[derive(Default)]
struct WotRuntimeState {
    active_pubkey: Option<String>,
    active_follows: BTreeSet<String>,
    bootstrap_pushed: bool,
    graph: WotGraph,
}

#[derive(Serialize)]
struct WotBootstrapSnapshot {
    active_pubkey: Option<String>,
    active_follow_count: usize,
    bootstrap_requested: bool,
    graph_follow_authors: usize,
    graph_mute_authors: usize,
}

impl WotBootstrapRuntime {
    /// Construct a runtime around the active-key slot and actor command sender.
    #[must_use]
    pub fn new(local_keys: ActiveLocalKeysSlot, tx: Sender<ActorCommand>) -> Self {
        Self {
            local_keys,
            tx,
            state: Mutex::new(WotRuntimeState::default()),
        }
    }

    /// Tick account-change cleanup and expose a small diagnostic snapshot.
    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        let active = self.active_pubkey();
        let snapshot = {
            let mut state = match self.state.lock() {
                Ok(state) => state,
                Err(_) => return serde_json::Value::Null,
            };
            if state.active_pubkey != active {
                if state.bootstrap_pushed {
                    self.withdraw_bootstrap();
                }
                state.active_pubkey = active.clone();
                state.active_follows.clear();
                state.bootstrap_pushed = false;
            }
            WotBootstrapSnapshot {
                active_pubkey: state.active_pubkey.clone(),
                active_follow_count: state.active_follows.len(),
                bootstrap_requested: state.bootstrap_pushed,
                graph_follow_authors: state.graph.follow_author_count(),
                graph_mute_authors: state.graph.mute_author_count(),
            }
        };
        serde_json::to_value(snapshot).unwrap_or(serde_json::Value::Null)
    }

    fn active_pubkey(&self) -> Option<String> {
        self.local_keys
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|keys| keys.public_key().to_hex()))
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
        let _ = self.tx.send(ActorCommand::PushInterest(interest));
    }

    fn withdraw_bootstrap(&self) {
        let _ = self.tx.send(ActorCommand::WithdrawInterest(
            active_follow_graph_interest_id(),
        ));
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
mod tests {
    use super::*;
    use nmp_core::planner::InterestLifecycle;
    use nmp_core::slots::new_active_local_keys_slot;
    use nostr::Keys;

    fn author(n: u16) -> String {
        format!("{n:064x}")
    }

    fn active_slot(keys: &Keys) -> ActiveLocalKeysSlot {
        let slot = new_active_local_keys_slot();
        *slot.lock().unwrap() = Some(keys.clone());
        slot
    }

    fn contact_event(event_author: &str, follows: usize) -> KernelEvent {
        KernelEvent {
            id: nmp_core::substrate::EventId::from("1".repeat(64)),
            author: event_author.to_string(),
            kind: KIND_CONTACT_LIST,
            created_at: 1_000,
            tags: (0..follows)
                .map(|i| vec!["p".to_string(), author(i as u16)])
                .collect(),
            content: String::new(),
        }
    }

    #[test]
    fn active_kind3_pushes_large_one_shot_wot_interest() {
        let keys = Keys::generate();
        let active = keys.public_key().to_hex();
        let (tx, rx) = std::sync::mpsc::channel();
        let runtime = WotBootstrapRuntime::new(active_slot(&keys), tx);

        runtime.on_kernel_event(&contact_event(&active, 1_052));

        let cmd = rx.recv().expect("wot bootstrap command");
        let ActorCommand::PushInterest(interest) = cmd else {
            panic!("expected PushInterest");
        };
        assert_eq!(interest.id, active_follow_graph_interest_id());
        assert!(matches!(interest.lifecycle, InterestLifecycle::OneShot));
        assert_eq!(interest.shape.limit, None);
        assert_eq!(interest.shape.authors.len(), 1_052);
        assert_eq!(
            interest.shape.kinds.into_iter().collect::<Vec<_>>(),
            crate::interest::WOT_BOOTSTRAP_KINDS
        );
    }

    #[test]
    fn account_switch_snapshot_withdraws_previous_bootstrap() {
        let keys = Keys::generate();
        let active = keys.public_key().to_hex();
        let (tx, rx) = std::sync::mpsc::channel();
        let slot = active_slot(&keys);
        let runtime = WotBootstrapRuntime::new(Arc::clone(&slot), tx);

        runtime.on_kernel_event(&contact_event(&active, 30));
        let _ = rx.recv().expect("initial push");
        *slot.lock().unwrap() = None;
        let _ = runtime.snapshot_json();

        let cmd = rx.recv().expect("withdraw command");
        let ActorCommand::WithdrawInterest(id) = cmd else {
            panic!("expected WithdrawInterest");
        };
        assert_eq!(id, active_follow_graph_interest_id());
    }
}
