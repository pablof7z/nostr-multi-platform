//! Chirp NIP-17 DM runtime wiring.
//!
//! This module is the thin host-shell glue around the protocol-side state
//! machine in [`nmp_nip17::dm_runtime`]: it registers the inbox projection,
//! exposes the `nmp.nip17.dm_relay_list` snapshot projection, and translates
//! [`DmRuntimeEffect`]s into [`ActorCommand`]s on the kernel actor. The
//! reconciliation decisions live in the protocol crate (D0 — host shells
//! stay logic-free).

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use nmp_core::{
    ActorCommand, NmpApp, RawEventObserver, RelayEditRowsSlot, read_eligible_relay_urls,
};
use nmp_nip17::{
    DmInboxProjection, DmRuntimeEffect, DmRuntimeState, active_giftwrap_inbox_interest,
    active_giftwrap_inbox_interest_id,
};
use serde::Serialize;

pub(crate) fn register_dm_runtime(app: &NmpApp) {
    register_inbox_projection(app);

    let controller = Arc::new(DmRuntimeController {
        relay_rows: app.relay_edit_rows_handle(),
        local_keys: app.nip17_local_keys(),
        tx: app.actor_sender(),
        state: Mutex::new(DmRuntimeState::default()),
    });
    app.register_snapshot_projection("nmp.nip17.dm_relay_list", move || controller.snapshot_json());
}

fn register_inbox_projection(app: &NmpApp) {
    let projection = Arc::new(DmInboxProjection::new(app.nip17_local_keys()));
    let observer_id = app.register_raw_event_observer(
        DmInboxProjection::kind_filter(),
        Arc::clone(&projection) as Arc<dyn RawEventObserver>,
    );
    if observer_id.0 == 0 {
        return;
    }
    if let Some(prev) = app.swap_nip17_dm_inbox_observer(Some(observer_id)) {
        app.unregister_raw_event_observer(prev);
    }
    app.register_snapshot_projection("nmp.nip17.dm_inbox", move || projection.snapshot_json());
}

struct DmRuntimeController {
    // PR-I: the kernel relay-edit slot is now a typed
    // [`RelayEditRowsSlot`] (`Arc<Mutex<RelayEditRowList>>`). We read via
    // `guard.as_slice()` so the inner `Vec<RelayEditRow>` never leaks
    // through this consumer's call sites.
    relay_rows: RelayEditRowsSlot,
    local_keys: Arc<Mutex<Option<nostr::Keys>>>,
    tx: Sender<ActorCommand>,
    state: Mutex<DmRuntimeState>,
}

impl DmRuntimeController {
    fn snapshot_json(&self) -> serde_json::Value {
        let active_pubkey = self.active_pubkey();
        let read_relay_urls = self.read_relay_urls();
        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            for effect in state.reconcile(active_pubkey.as_deref(), &read_relay_urls) {
                self.apply(effect);
            }
        }
        serde_json::to_value(DmRelayListSnapshot {
            active_pubkey,
            read_relay_urls,
        })
        .unwrap_or_else(|_| serde_json::json!({ "active_pubkey": null, "read_relay_urls": [] }))
    }

    fn active_pubkey(&self) -> Option<String> {
        self.local_keys
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|keys| keys.public_key().to_hex()))
    }

    fn read_relay_urls(&self) -> Vec<String> {
        // PR-I: typed slot — iterate via `as_slice()` so we never touch
        // the inner `Vec<RelayEditRow>` directly.
        self.relay_rows
            .lock()
            .map(|rows| read_eligible_relay_urls(rows.as_slice()))
            .unwrap_or_default()
    }

    fn apply(&self, effect: DmRuntimeEffect) {
        let cmd = match effect {
            DmRuntimeEffect::PushInboxInterest(pubkey) => {
                ActorCommand::PushInterest(active_giftwrap_inbox_interest(&pubkey))
            }
            DmRuntimeEffect::WithdrawInboxInterest => {
                ActorCommand::WithdrawInterest(active_giftwrap_inbox_interest_id())
            }
            DmRuntimeEffect::PublishRelayList { event, .. } => {
                // This DmRuntime effect is the non-dispatch internal path (the
                // host-driven Chirp DM runtime), not an `ActionModule::execute`
                // call. `None` matches the prior behaviour — the action seam
                // at `nmp_nip17::PublishDmRelayListAction::execute` is where
                // the correlation_id round-trip happens.
                ActorCommand::PublishUnsignedEvent {
                    event,
                    correlation_id: None,
                }
            }
        };
        let _ = self.tx.send(cmd);
    }
}

#[derive(Serialize)]
struct DmRelayListSnapshot {
    active_pubkey: Option<String>,
    read_relay_urls: Vec<String>,
}
