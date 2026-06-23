//! Behaviour-preserving unit tests for the re-homed NIP-57 zap-subscription
//! reconciler ([`super::ZapReceiptsRuntimeController`]).
//!
//! The reconciler moved OFF the dynamic snapshot-projection registry (which it
//! abused with a `Value::Null` projection purely to obtain a per-tick callback)
//! ONTO the generic `AppHost::register_snapshot_tick_observer` seam. Its
//! Push/Withdraw logic is unchanged — these tests pin that logic by driving the
//! same `tick()` the observer now calls and asserting the emitted
//! [`ActorCommand`] sequence across sign-in / account-switch / sign-out.
//!
//! The `"nmp.nip57.zap_subscription"` key is no longer a projection at all; the
//! end-to-end "absent from the JSON projections map" proof lives in the FFI
//! integration test (`tests/register_defaults.rs`).

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use nmp_core::slots::ActiveAccountSlot;
use nmp_core::actor::ActorCommand;
use nmp_core::actor::{InterestsCommand};
use nmp_nip57::{self_zap_receipts_interest, self_zap_receipts_interest_id};
use nostr::Keys;

use super::ZapReceiptsRuntimeController;

/// Build a controller wired to a fresh in-memory actor channel + a shared
/// pubkey-only active-account slot the test mutates to simulate sign-in /
/// sign-out. The slot carries the hex pubkey only (Finding C) — the same shape
/// the kernel populates for EVERY backend, bunker included — so these tests
/// exercise the bunker-safe activation path by construction.
fn controller() -> (
    ZapReceiptsRuntimeController,
    ActiveAccountSlot,
    Receiver<nmp_core::ActorMail>,
) {
    let (inbox_tx, rx) = mpsc::channel::<nmp_core::ActorMail>();
    let tx = nmp_core::CommandSender::new(inbox_tx);
    let active_pubkey: ActiveAccountSlot = Arc::new(Mutex::new(None));
    let controller = ZapReceiptsRuntimeController {
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
/// self-zap-receipts subscription. The reconciler reads the pubkey-only slot
/// the kernel populates for every backend; with NO secret keys ever present it
/// must still push the kind:9735 `#p` interest for the active pubkey.
#[test]
fn bunker_only_account_activates_self_zap_receipts() {
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
        "a bunker account must still push exactly one zap-receipts interest"
    );
    assert_push_for(&cmds[0], &pubkey);
}

fn assert_push_for(cmd: &ActorCommand, pubkey: &str) {
    match cmd {
        ActorCommand::Interests(InterestsCommand::PushInterest(interest)) => {
            assert_eq!(
                interest.id,
                self_zap_receipts_interest(pubkey).id,
                "pushed interest must be the active-pubkey zap-receipts interest"
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
                self_zap_receipts_interest_id(),
                "withdraw must target the pubkey-invariant zap-receipts interest id"
            );
        }
        other => panic!("expected WithdrawInterest, got {other:?}"),
    }
}
