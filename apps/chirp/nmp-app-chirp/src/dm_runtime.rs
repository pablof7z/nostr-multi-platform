//! Chirp NIP-17 DM runtime wiring.
//!
//! This module owns the protocol orchestration that Swift used to mirror:
//! registering the inbox projection, keeping the active account's gift-wrap
//! interest current, and publishing the account's kind:10050 DM relay list
//! when the read-eligible relay set changes.

use std::collections::BTreeSet;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::UnsignedEvent;
use nmp_core::{
    ActorCommand, NmpApp, RawEventObserver, RelayEditRowsSlot, read_eligible_relay_urls,
};
use nmp_nip17::{
    DmInboxProjection, active_giftwrap_inbox_interest, active_giftwrap_inbox_interest_id,
    build_dm_relay_list_event,
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
    app.register_snapshot_projection("nip17.dm_relay_list", move || controller.snapshot_value());
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
    app.register_snapshot_projection("nip17.dm_inbox", move || projection.snapshot_json());
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
    fn snapshot_value(&self) -> serde_json::Value {
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
        .unwrap_or(serde_json::Value::Null)
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
                ActorCommand::PublishUnsignedEventToRelays {
                    event,
                    relays: Vec::new(),
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

#[derive(Default)]
struct DmRuntimeState {
    last_inbox_pubkey: Option<String>,
    last_published: Option<(String, BTreeSet<String>)>,
}

impl DmRuntimeState {
    fn reconcile(
        &mut self,
        active_pubkey: Option<&str>,
        read_relay_urls: &[String],
    ) -> Vec<DmRuntimeEffect> {
        let mut effects = Vec::new();
        let active_pubkey = active_pubkey.filter(|pk| !pk.is_empty());
        let Some(account) = active_pubkey else {
            if self.last_inbox_pubkey.take().is_some() {
                effects.push(DmRuntimeEffect::WithdrawInboxInterest);
            }
            self.last_published = None;
            return effects;
        };

        if self.last_inbox_pubkey.as_deref() != Some(account) {
            self.last_inbox_pubkey = Some(account.to_string());
            effects.push(DmRuntimeEffect::PushInboxInterest(account.to_string()));
        }

        if self
            .last_published
            .as_ref()
            .is_some_and(|(published_account, _)| published_account != account)
        {
            self.last_published = None;
        }

        let event = build_dm_relay_list_event(read_relay_urls);
        let relay_urls = relay_urls_from_event(&event);
        if relay_urls.is_empty() {
            return effects;
        }
        let relay_set = relay_urls.into_iter().collect::<BTreeSet<_>>();
        if self
            .last_published
            .as_ref()
            .is_some_and(|(published_account, published_set)| {
                published_account == account && published_set == &relay_set
            })
        {
            return effects;
        }

        self.last_published = Some((account.to_string(), relay_set.clone()));
        effects.push(DmRuntimeEffect::PublishRelayList { event, relay_set });
        effects
    }
}

#[derive(Debug, PartialEq, Eq)]
enum DmRuntimeEffect {
    PushInboxInterest(String),
    WithdrawInboxInterest,
    PublishRelayList {
        event: UnsignedEvent,
        relay_set: BTreeSet<String>,
    },
}

fn relay_urls_from_event(event: &UnsignedEvent) -> Vec<String> {
    event
        .tags
        .iter()
        .filter_map(|tag| match tag.as_slice() {
            [marker, url] if marker == "relay" => Some(url.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relays(urls: &[&str]) -> Vec<String> {
        urls.iter().map(|url| url.to_string()).collect()
    }

    #[test]
    fn active_account_pushes_interest_and_publishes_once() {
        let mut state = DmRuntimeState::default();
        let effects = state.reconcile(Some("alice"), &relays(&["wss://a.example"]));
        assert!(matches!(
            effects.as_slice(),
            [
                DmRuntimeEffect::PushInboxInterest(pk),
                DmRuntimeEffect::PublishRelayList { relay_set, .. }
            ] if pk == "alice" && relay_set.contains("wss://a.example")
        ));
        assert!(
            state
                .reconcile(Some("alice"), &relays(&["wss://a.example"]))
                .is_empty()
        );
    }

    #[test]
    fn relay_set_changes_republish_without_repush_interest() {
        let mut state = DmRuntimeState::default();
        let _ = state.reconcile(Some("alice"), &relays(&["wss://a.example"]));
        let effects = state.reconcile(
            Some("alice"),
            &relays(&["wss://a.example", "wss://b.example"]),
        );
        assert!(matches!(
            effects.as_slice(),
            [DmRuntimeEffect::PublishRelayList { relay_set, .. }]
                if relay_set.contains("wss://a.example")
                    && relay_set.contains("wss://b.example")
        ));
    }

    #[test]
    fn account_switch_replaces_interest_and_republishes_same_relays() {
        let mut state = DmRuntimeState::default();
        let _ = state.reconcile(Some("alice"), &relays(&["wss://a.example"]));
        let effects = state.reconcile(Some("bob"), &relays(&["wss://a.example"]));
        assert!(matches!(
            effects.as_slice(),
            [
                DmRuntimeEffect::PushInboxInterest(pk),
                DmRuntimeEffect::PublishRelayList { relay_set, .. }
            ] if pk == "bob" && relay_set.contains("wss://a.example")
        ));
    }

    #[test]
    fn logout_withdraws_active_interest_slot() {
        let mut state = DmRuntimeState::default();
        let _ = state.reconcile(Some("alice"), &relays(&["wss://a.example"]));
        assert_eq!(
            state.reconcile(None, &relays(&["wss://a.example"])),
            vec![DmRuntimeEffect::WithdrawInboxInterest]
        );
    }
}
