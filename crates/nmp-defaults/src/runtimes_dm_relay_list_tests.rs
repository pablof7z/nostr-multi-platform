//! Tick-observer seam tests for the NIP-17 DM relay-list reconciler
//! ([`super::DmRuntimeController`]) — Finding C + Blocker A.
//!
//! The reconciler must:
//!
//! 1. Activate for a remote-signer (bunker) account (Finding C): the kernel
//!    populates the pubkey-only `ActiveAccountSlot` for EVERY backend, while
//!    `active_local_keys()` stays `None` for bunker. Tests drive the controller
//!    directly (same pattern as `runtimes_zap_tests.rs`) with a pubkey-only slot
//!    and assert the active-account gift-wrap inbox interest is pushed — proving
//!    the reconciler reads identity, never secret key material.
//!
//! 2. Fire from the **tick observer** seam, NOT from the projection closure
//!    (Blocker A): the `DmRuntimeController::tick` method must emit the inbox
//!    interest push; a pure-read call to `typed_relay_list` must NOT emit effects.
//!    This proves reconciliation cannot be lost by dropping the JSON projection
//!    lane.

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use nmp_core::slots::ActiveAccountSlot;
use nmp_core::{AppRelayList};
use nmp_core::actor::{ActorCommand};
use nmp_core::actor::{InterestsCommand};
use nmp_nip17::{active_giftwrap_inbox_interest_id, DmRuntimeState};
use nostr::Keys;

use super::DmRuntimeController;

/// Build a controller wired to a fresh actor channel and a pubkey-only active
/// account slot. The relay slot is empty — the gift-wrap inbox-interest push
/// (the bunker-activation signal under test) fires from the active pubkey
/// alone, independent of any configured relay list. The slot carries hex
/// pubkey only — the bunker shape.
fn controller() -> (DmRuntimeController, ActiveAccountSlot, Receiver<nmp_core::ActorMail>) {
    let (inbox_tx, rx) = mpsc::channel::<nmp_core::ActorMail>();
    let tx = nmp_core::CommandSender::new(inbox_tx);
    let active_pubkey: ActiveAccountSlot = Arc::new(Mutex::new(None));
    let relay_slot = Arc::new(Mutex::new(AppRelayList::default()));
    let controller = DmRuntimeController {
        relay_slot,
        active_pubkey: Arc::clone(&active_pubkey),
        tx,
        state: Mutex::new(DmRuntimeState::default()),
    };
    (controller, active_pubkey, rx)
}

/// Drain whatever the controller enqueued this tick (ADR-0050 §D3a: the inbox
/// carries `ActorMail`).
fn drained(rx: &Receiver<nmp_core::ActorMail>) -> Vec<ActorCommand> {
    rx.try_iter()
        .map(|mail| match mail {
            nmp_core::ActorMail::Command(cmd) => cmd,
            other => panic!("expected ActorMail::Command, got {other:?}"),
        })
        .collect()
}

/// Finding C — a bunker (remote-signer-only) account must activate the DM
/// relay-list runtime. With ONLY the hex pubkey present (no secret keys), the
/// reconciler must push the active-account gift-wrap inbox interest.
///
/// Blocker A seam proof: the push is driven by `DmRuntimeController::tick`
/// (the tick observer), NOT by the projection closure. The tick observer is
/// the ONLY path that calls `state.reconcile(...)` → `apply(...)`.
#[test]
fn bunker_only_account_activates_dm_relay_list_runtime() {
    let (controller, slot, rx) = controller();
    let keys = Keys::generate();

    // Pubkey-only sign-in (bunker shape): hex pubkey present, zero secrets.
    *slot.lock().unwrap() = Some(keys.public_key().to_hex());

    // `tick` drives reconciliation (push/withdraw) once per tick — this is
    // the tick-observer seam, NOT the projection closure.
    controller.tick();

    let cmds: Vec<ActorCommand> = drained(&rx);
    let pushed_inbox = cmds.iter().any(|cmd| {
        matches!(
            cmd,
            ActorCommand::Interests(InterestsCommand::EnsureInterest { interest, .. })
                if interest.id == active_giftwrap_inbox_interest_id()
        )
    });
    assert!(
        pushed_inbox,
        "a bunker account must still push the DM gift-wrap inbox interest via tick(); got {cmds:?}"
    );
}

/// Cold start before sign-in (slot `None`) must enqueue no inbox interest.
#[test]
fn no_account_enqueues_no_inbox_interest() {
    let (controller, _slot, rx) = controller();
    controller.tick();
    let cmds: Vec<ActorCommand> = drained(&rx);
    assert!(
        !cmds.iter().any(|cmd| matches!(
            cmd,
            ActorCommand::Interests(InterestsCommand::EnsureInterest { interest, .. })
                if interest.id == active_giftwrap_inbox_interest_id()
        )),
        "no signed-in account must not push an inbox interest; got {cmds:?}"
    );
}

/// Blocker A — prove the tick observer seam drives reconciliation and the
/// pure-read path (`typed_relay_list`) does NOT emit effects.
///
/// This test would FAIL if `tick()` were removed or if reconciliation were
/// moved into the projection closure (`typed_relay_list`).
#[test]
fn reconciliation_fires_from_tick_observer_not_from_projection_read() {
    let (controller, slot, rx) = controller();
    let keys = Keys::generate();
    *slot.lock().unwrap() = Some(keys.public_key().to_hex());

    // A pure read of the relay list must NOT trigger effects.
    let _relay_list = controller.typed_relay_list();
    let cmds_after_read: Vec<ActorCommand> = drained(&rx);
    assert!(
        cmds_after_read.is_empty(),
        "typed_relay_list (pure read) must not emit actor commands; got {cmds_after_read:?}"
    );

    // Only tick() emits the push — this is the tick-observer seam.
    controller.tick();
    let cmds_after_tick: Vec<ActorCommand> = drained(&rx);
    let pushed = cmds_after_tick.iter().any(|cmd| {
        matches!(
            cmd,
            ActorCommand::Interests(InterestsCommand::EnsureInterest { interest, .. })
                if interest.id == active_giftwrap_inbox_interest_id()
        )
    });
    assert!(
        pushed,
        "tick() must emit the inbox-interest push; got {cmds_after_tick:?}"
    );

    // A second tick with the same pubkey must NOT re-push (idempotent after
    // the state machine has already pushed for this pubkey).
    controller.tick();
    let cmds_second_tick: Vec<ActorCommand> = drained(&rx);
    let pushed_again = cmds_second_tick.iter().any(|cmd| {
        matches!(
            cmd,
            ActorCommand::Interests(InterestsCommand::EnsureInterest { interest, .. })
                if interest.id == active_giftwrap_inbox_interest_id()
        )
    });
    assert!(
        !pushed_again,
        "a second tick with the same pubkey must NOT re-push; got {cmds_second_tick:?}"
    );
}
