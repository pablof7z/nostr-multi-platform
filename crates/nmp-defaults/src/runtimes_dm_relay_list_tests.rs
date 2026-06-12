//! Bunker-activation tests for the NIP-17 DM relay-list reconciler
//! ([`super::DmRuntimeController`]) — Finding C.
//!
//! The reconciler must activate for a remote-signer (bunker) account: the
//! kernel populates the pubkey-only `ActiveAccountSlot` for EVERY backend,
//! while `active_local_keys()` stays `None` for bunker. These tests drive the
//! controller directly (same pattern as `runtimes_zap_tests.rs`) with a
//! pubkey-only slot and assert the active-account gift-wrap inbox interest is
//! pushed — proving the reconciler reads identity, never secret key material.

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use nmp_core::slots::ActiveAccountSlot;
use nmp_core::{ActorCommand, AppRelayList};
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
#[test]
fn bunker_only_account_activates_dm_relay_list_runtime() {
    let (controller, slot, rx) = controller();
    let keys = Keys::generate();

    // Pubkey-only sign-in (bunker shape): hex pubkey present, zero secrets.
    *slot.lock().unwrap() = Some(keys.public_key().to_hex());

    // `snapshot_json` drives reconciliation (push/withdraw) once per tick.
    let _ = controller.snapshot_json();

    let cmds: Vec<ActorCommand> = drained(&rx);
    let pushed_inbox = cmds.iter().any(|cmd| {
        matches!(
            cmd,
            ActorCommand::PushInterest(interest)
                if interest.id == active_giftwrap_inbox_interest_id()
        )
    });
    assert!(
        pushed_inbox,
        "a bunker account must still push the DM gift-wrap inbox interest; got {cmds:?}"
    );
}

/// Cold start before sign-in (slot `None`) must enqueue no inbox interest.
#[test]
fn no_account_enqueues_no_inbox_interest() {
    let (controller, _slot, rx) = controller();
    let _ = controller.snapshot_json();
    let cmds: Vec<ActorCommand> = drained(&rx);
    assert!(
        !cmds.iter().any(|cmd| matches!(
            cmd,
            ActorCommand::PushInterest(interest)
                if interest.id == active_giftwrap_inbox_interest_id()
        )),
        "no signed-in account must not push an inbox interest; got {cmds:?}"
    );
}
