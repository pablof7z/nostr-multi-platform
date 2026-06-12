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

use nmp_core::ActorCommand;
use nmp_nip57::{self_zap_receipts_interest, self_zap_receipts_interest_id};
use nostr::Keys;

use super::ZapReceiptsRuntimeController;

/// Build a controller wired to a fresh in-memory actor channel + a shared
/// active-local-keys slot the test mutates to simulate sign-in / sign-out.
fn controller() -> (
    ZapReceiptsRuntimeController,
    Arc<Mutex<Option<Keys>>>,
    Receiver<ActorCommand>,
) {
    let (tx, rx) = mpsc::channel();
    let local_keys = Arc::new(Mutex::new(None));
    let controller = ZapReceiptsRuntimeController {
        local_keys: Arc::clone(&local_keys),
        tx,
        last_pushed_pubkey: Mutex::new(None),
    };
    (controller, local_keys, rx)
}

/// Drain whatever the controller enqueued this tick.
fn drained(rx: &Receiver<ActorCommand>) -> Vec<ActorCommand> {
    rx.try_iter().collect()
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
    *slot.lock().unwrap() = Some(keys);
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

    *slot.lock().unwrap() = Some(first);
    controller.tick();
    let _ = drained(&rx); // first sign-in push already proven above

    *slot.lock().unwrap() = Some(second);
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
    *slot.lock().unwrap() = Some(Keys::generate());
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

fn assert_push_for(cmd: &ActorCommand, pubkey: &str) {
    match cmd {
        ActorCommand::PushInterest(interest) => {
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
        ActorCommand::WithdrawInterest(id) => {
            assert_eq!(
                *id,
                self_zap_receipts_interest_id(),
                "withdraw must target the pubkey-invariant zap-receipts interest id"
            );
        }
        other => panic!("expected WithdrawInterest, got {other:?}"),
    }
}
