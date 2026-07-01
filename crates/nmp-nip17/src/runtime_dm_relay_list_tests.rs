//! Event-observer seam tests for the NIP-17 DM relay-list reconciler.

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use nmp_core::actor::{ActorCommand, InterestsCommand};
use nmp_core::slots::ActiveAccountSlot;
use nmp_core::AppRelayList;
use nostr::Keys;

use crate::{
    active_giftwrap_inbox_identity, active_giftwrap_inbox_interest_id, peer_dm_relay_list_identity,
    DmInboxProjection, DmRuntimeEffect, DmRuntimeState,
};

use super::DmRuntimeController;

fn controller() -> (
    DmRuntimeController,
    ActiveAccountSlot,
    Receiver<nmp_core::ActorMail>,
) {
    let (inbox_tx, rx) = mpsc::channel::<nmp_core::ActorMail>();
    let projection_tx = nmp_core::CommandSender::new(inbox_tx.clone());
    let tx = nmp_core::CommandSender::new(inbox_tx);
    let active_pubkey: ActiveAccountSlot = Arc::new(Mutex::new(None));
    let relay_slot = Arc::new(Mutex::new(AppRelayList::default()));
    let inbox_projection = Arc::new(DmInboxProjection::new(
        projection_tx,
        Arc::clone(&active_pubkey),
    ));
    let controller = DmRuntimeController {
        relay_slot,
        active_pubkey: Arc::clone(&active_pubkey),
        event_store: nmp_core::slots::new_event_store_slot(),
        tx,
        state: Mutex::new(DmRuntimeState::default()),
        inbox_projection,
    };
    (controller, active_pubkey, rx)
}

fn drained(rx: &Receiver<nmp_core::ActorMail>) -> Vec<ActorCommand> {
    rx.try_iter()
        .map(|mail| match mail {
            nmp_core::ActorMail::Command(cmd) => cmd,
            other => panic!("expected ActorMail::Command, got {other:?}"),
        })
        .collect()
}

#[test]
fn bunker_only_account_activates_dm_relay_list_runtime() {
    let (controller, slot, rx) = controller();
    let keys = Keys::generate();

    *slot.lock().unwrap() = Some(keys.public_key().to_hex());
    controller.sync();

    let cmds: Vec<ActorCommand> = drained(&rx);
    let pushed_inbox = cmds.iter().any(|cmd| {
        matches!(
            cmd,
            ActorCommand::Interests(InterestsCommand::EnsureInterest { identity, interest })
                if *identity == active_giftwrap_inbox_identity()
                    && interest.id == active_giftwrap_inbox_interest_id()
        )
    });
    assert!(
        pushed_inbox,
        "a bunker account must ensure the DM gift-wrap inbox interest via sync(); got {cmds:?}"
    );
}

#[test]
fn no_account_enqueues_no_inbox_interest() {
    let (controller, _slot, rx) = controller();
    controller.sync();
    let cmds: Vec<ActorCommand> = drained(&rx);
    assert!(
        !cmds.iter().any(|cmd| matches!(
            cmd,
            ActorCommand::Interests(InterestsCommand::EnsureInterest { identity, interest })
                if *identity == active_giftwrap_inbox_identity()
                    && interest.id == active_giftwrap_inbox_interest_id()
        )),
        "no signed-in account must not ensure an inbox interest; got {cmds:?}"
    );
}

#[test]
fn reconciliation_fires_from_event_observer_not_from_projection_read() {
    let (controller, slot, rx) = controller();
    let keys = Keys::generate();
    *slot.lock().unwrap() = Some(keys.public_key().to_hex());

    let _relay_list = controller.typed_relay_list();
    let cmds_after_read: Vec<ActorCommand> = drained(&rx);
    assert!(
        cmds_after_read.is_empty(),
        "typed_relay_list (pure read) must not emit actor commands; got {cmds_after_read:?}"
    );

    controller.sync();
    let cmds_after_tick: Vec<ActorCommand> = drained(&rx);
    let pushed = cmds_after_tick.iter().any(|cmd| {
        matches!(
            cmd,
            ActorCommand::Interests(InterestsCommand::EnsureInterest { identity, interest })
                if *identity == active_giftwrap_inbox_identity()
                    && interest.id == active_giftwrap_inbox_interest_id()
        )
    });
    assert!(
        pushed,
        "sync() must emit the inbox-interest ensure; got {cmds_after_tick:?}"
    );

    controller.sync();
    let cmds_second_tick: Vec<ActorCommand> = drained(&rx);
    let pushed_again = cmds_second_tick.iter().any(|cmd| {
        matches!(
            cmd,
            ActorCommand::Interests(InterestsCommand::EnsureInterest { identity, interest })
                if *identity == active_giftwrap_inbox_identity()
                    && interest.id == active_giftwrap_inbox_interest_id()
        )
    });
    assert!(
        !pushed_again,
        "a second sync with the same pubkey must not re-ensure; got {cmds_second_tick:?}"
    );
}

#[test]
fn peer_relay_list_effect_maps_to_kind10050_interest() {
    let (controller, _slot, rx) = controller();
    let peer = Keys::generate().public_key().to_hex();

    controller.apply(DmRuntimeEffect::PushPeerRelayListInterest(peer.clone()));

    let cmds: Vec<ActorCommand> = drained(&rx);
    let pushed_peer = cmds.iter().any(|cmd| {
        matches!(
            cmd,
            ActorCommand::Interests(InterestsCommand::EnsureInterest { identity, interest })
                if *identity == peer_dm_relay_list_identity(&peer)
                    && interest.shape.authors.contains(&peer)
                    && interest.shape.kinds.contains(&10050)
                    && interest.shape.limit == Some(1)
                    && interest.is_indexer_discovery
        )
    });
    assert!(
        pushed_peer,
        "peer relay-list hydration must use a Rust-owned kind:10050 interest; got {cmds:?}"
    );
}

#[test]
fn own_relay_list_effect_maps_to_kind10050_interest() {
    let (controller, _slot, rx) = controller();
    let account = Keys::generate().public_key().to_hex();

    controller.apply(DmRuntimeEffect::PushOwnRelayListInterest(account.clone()));

    let cmds: Vec<ActorCommand> = drained(&rx);
    let pushed_self = cmds.iter().any(|cmd| {
        matches!(
            cmd,
            ActorCommand::Interests(InterestsCommand::EnsureInterest { identity, interest })
                if *identity == peer_dm_relay_list_identity(&account)
                    && interest.shape.authors.contains(&account)
                    && interest.shape.kinds.contains(&10050)
                    && interest.shape.limit == Some(1)
                    && interest.is_indexer_discovery
        )
    });
    assert!(
        pushed_self,
        "own relay-list hydration must use a Rust-owned kind:10050 interest for NIP-17 self-copy routing; got {cmds:?}"
    );
}
