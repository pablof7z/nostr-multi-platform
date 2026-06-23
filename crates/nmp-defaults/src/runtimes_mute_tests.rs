//! Behaviour-preserving unit tests for the NIP-51 mute-list subscription
//! reconciler ([`super::mute_runtime::MuteRuntimeController`]).
//!
//! Mirrors `runtimes_zap_tests.rs` line-for-line, but for mute-list interests
//! (kind:10000, `authors=[active_pubkey]`). Drives the same `tick()` the
//! tick observer calls and asserts the emitted [`ActorCommand`] sequence across
//! sign-in / account-switch / sign-out.

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use nmp_core::slots::ActiveAccountSlot;
use nmp_core::actor::ActorCommand;
use nmp_core::InterestsCommand;
use nmp_nip51::{active_mute_list_interest, active_mute_list_interest_id};
use nostr::Keys;

use super::mute_runtime::MuteRuntimeController;

/// Build a controller wired to a fresh in-memory actor channel + a shared
/// pubkey-only active-account slot the test mutates to simulate sign-in /
/// sign-out. The slot carries the hex pubkey only (Finding C) — the same shape
/// the kernel populates for EVERY backend, bunker included.
fn controller() -> (
    MuteRuntimeController,
    ActiveAccountSlot,
    Receiver<nmp_core::ActorMail>,
) {
    let (inbox_tx, rx) = mpsc::channel::<nmp_core::ActorMail>();
    let tx = nmp_core::CommandSender::new(inbox_tx);
    let active_pubkey: ActiveAccountSlot = Arc::new(Mutex::new(None));
    let controller = MuteRuntimeController {
        active_pubkey: Arc::clone(&active_pubkey),
        tx,
        last_pushed_pubkey: Mutex::new(None),
    };
    (controller, active_pubkey, rx)
}

/// Set the pubkey-only slot to `keys`' hex pubkey (no secret material), the
/// shape a bunker / remote-signer account presents.
fn sign_in(slot: &ActiveAccountSlot, keys: &Keys) {
    *slot.lock().unwrap() = Some(keys.public_key().to_hex());
}

/// Drain whatever the controller enqueued this tick.
fn drained(rx: &Receiver<nmp_core::ActorMail>) -> Vec<ActorCommand> {
    rx.try_iter()
        .map(|mail| match mail {
            nmp_core::ActorMail::Command(cmd) => cmd,
            other => panic!("expected ActorMail::Command, got {other:?}"),
        })
        .collect()
}

/// Sign-in pushes exactly one `PushInterest` for the active pubkey; a steady
/// tick afterwards enqueues nothing (the common fast path).
#[test]
fn sign_in_pushes_interest_once_then_idles() {
    let (controller, slot, rx) = controller();
    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();

    // Cold start before sign-in: no actor traffic.
    controller.tick();
    assert!(drained(&rx).is_empty(), "cold start must enqueue nothing");

    // Sign in → exactly one PushInterest for this pubkey.
    sign_in(&slot, &keys);
    controller.tick();
    let cmds = drained(&rx);
    assert_eq!(cmds.len(), 1, "sign-in enqueues exactly one command");
    assert_push_for(&cmds[0], &pubkey);

    // Steady state: no change → no further traffic.
    controller.tick();
    assert!(
        drained(&rx).is_empty(),
        "no active-pubkey change must enqueue nothing"
    );
}

/// An account switch withdraws the standing interest (by its pubkey-invariant
/// id) then pushes the new account's interest — in that order, once.
#[test]
fn account_switch_withdraws_then_pushes() {
    let (controller, slot, rx) = controller();
    let first = Keys::generate();
    let second = Keys::generate();
    let second_pubkey = second.public_key().to_hex();

    sign_in(&slot, &first);
    controller.tick();
    let _ = drained(&rx); // first sign-in push already proven above

    sign_in(&slot, &second);
    controller.tick();
    let cmds = drained(&rx);
    assert_eq!(cmds.len(), 2, "switch enqueues withdraw + push");
    assert_withdraw(&cmds[0]);
    assert_push_for(&cmds[1], &second_pubkey);
}

/// Sign-out (active → none) withdraws the standing interest once, then a
/// subsequent idle tick enqueues nothing.
#[test]
fn sign_out_withdraws_interest_once() {
    let (controller, slot, rx) = controller();
    sign_in(&slot, &Keys::generate());
    controller.tick();
    let _ = drained(&rx);

    *slot.lock().unwrap() = None;
    controller.tick();
    let cmds = drained(&rx);
    assert_eq!(cmds.len(), 1, "sign-out enqueues exactly one command");
    assert_withdraw(&cmds[0]);

    controller.tick();
    assert!(
        drained(&rx).is_empty(),
        "already signed out → no further traffic"
    );
}

/// Finding C — a bunker (remote-signer-only) account must activate the
/// mute-list subscription. The reconciler reads the pubkey-only slot the
/// kernel populates for every backend; with NO secret keys ever present it
/// must still push the kind:10000 `authors` interest for the active pubkey.
#[test]
fn bunker_only_account_activates_mute_list_interest() {
    let (controller, slot, rx) = controller();
    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();

    // Pubkey-only sign-in (bunker shape): hex pubkey present, zero secrets.
    sign_in(&slot, &keys);
    controller.tick();

    let cmds = drained(&rx);
    assert_eq!(
        cmds.len(),
        1,
        "a bunker account must still push exactly one mute-list interest"
    );
    assert_push_for(&cmds[0], &pubkey);
}

fn assert_push_for(cmd: &ActorCommand, pubkey: &str) {
    match cmd {
        ActorCommand::Interests(InterestsCommand::PushInterest(interest)) => {
            assert_eq!(
                interest.id,
                active_mute_list_interest(pubkey).id,
                "pushed interest must be the active-pubkey mute-list interest"
            );
        }
        other => panic!("expected PushInterest, got {other:?}"),
    }
}

fn assert_withdraw(cmd: &ActorCommand) {
    match cmd {
        ActorCommand::Interests(InterestsCommand::WithdrawInterest(id)) => {
            assert_eq!(
                *id,
                active_mute_list_interest_id(),
                "withdraw must target the pubkey-invariant mute-list interest id"
            );
        }
        other => panic!("expected WithdrawInterest, got {other:?}"),
    }
}
