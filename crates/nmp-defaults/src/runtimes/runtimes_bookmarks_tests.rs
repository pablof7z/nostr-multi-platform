//! Behaviour-preserving unit tests for the NIP-51 bookmark-list subscription
//! reconciler ([`super::BookmarksRuntimeController`]).
//!
//! Mirrors the NIP-57 zap-receipts tests in `../runtimes_zap_tests.rs` line
//! for line, exercising the same ensure/drop-owner logic across sign-in /
//! account-switch / sign-out / cold-start / bunker-account paths.

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;
use nmp_core::actor::InterestsCommand;
use nmp_core::slots::ActiveAccountSlot;
use nmp_nip51::{active_bookmark_list_identity, active_bookmark_list_interest_id};
use nmp_planner::{InterestLifecycle, InterestScope};
use nostr::Keys;

use super::BookmarksRuntimeController;

/// Build a controller wired to a fresh in-memory actor channel + a shared
/// pubkey-only active-account slot the test mutates to simulate sign-in /
/// sign-out. The slot carries the hex pubkey only (Finding C) — the same
/// shape the kernel populates for EVERY backend, bunker included.
fn controller() -> (
    BookmarksRuntimeController,
    ActiveAccountSlot,
    Receiver<nmp_core::ActorMail>,
) {
    let (inbox_tx, rx) = mpsc::channel::<nmp_core::ActorMail>();
    let tx = nmp_core::CommandSender::new(inbox_tx);
    let active_pubkey: ActiveAccountSlot = Arc::new(Mutex::new(None));
    let controller = BookmarksRuntimeController {
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

/// Sign-in ensures exactly one interest for the active pubkey; a steady tick
/// afterwards enqueues nothing (the common fast path).
#[test]
fn sign_in_pushes_interest_once_then_idles() {
    let (controller, slot, rx) = controller();
    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();

    // Cold start before sign-in: no actor traffic.
    controller.tick();
    assert!(drained(&rx).is_empty(), "cold start must enqueue nothing");

    // Sign in → exactly one interest for this pubkey.
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

/// An account switch drops the standing owner then ensures the new account's
/// interest — in that order, once.
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
    assert_eq!(cmds.len(), 2, "switch enqueues drop + ensure");
    assert_withdraw(&cmds[0]);
    assert_push_for(&cmds[1], &second_pubkey);
}

/// Sign-out (active → none) drops the standing owner once, then a
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
/// bookmark-list subscription. The reconciler reads the pubkey-only slot
/// the kernel populates for every backend; with NO secret keys ever present it
/// must still ensure the kind:10003 `authors=[pubkey]` interest.
#[test]
fn bunker_only_account_activates_bookmark_list_interest() {
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
        "a bunker account must still ensure exactly one bookmark-list interest"
    );
    assert_push_for(&cmds[0], &pubkey);
}

fn assert_push_for(cmd: &ActorCommand, pubkey: &str) {
    match cmd {
        ActorCommand::Interests(InterestsCommand::EnsureInterest { identity, interest }) => {
            assert_eq!(
                identity,
                &active_bookmark_list_identity(),
                "ensured interest must use the active bookmark-list owner"
            );
            // The id is pubkey-invariant, so checking it alone doesn't prove
            // the correct pubkey was embedded. Assert the full interest shape:
            // authors, kind, lifecycle, and scope — so a stale or hardcoded
            // author filter is caught here, not silently routed to the wrong
            // relay set.
            assert_eq!(
                interest.id,
                active_bookmark_list_interest_id(),
                "ensured interest must carry the pubkey-invariant slot id"
            );
            assert_eq!(
                interest.shape.authors,
                std::collections::BTreeSet::from([pubkey.to_string()]),
                "shape.authors must be EXACTLY {{active_pubkey}} — exactly one \
                 author, the active key; got {:?}",
                interest.shape.authors
            );
            assert_eq!(
                interest.shape.kinds,
                std::collections::BTreeSet::from([10003u32]),
                "shape.kinds must be EXACTLY {{10003}} — a future interest that \
                 added an extra kind must fail this gate; got {:?}",
                interest.shape.kinds
            );
            assert!(
                matches!(interest.lifecycle, InterestLifecycle::Tailing),
                "lifecycle must be Tailing; got {:?}",
                interest.lifecycle
            );
            assert!(
                matches!(interest.scope, InterestScope::Global),
                "scope must be Global; got {:?}",
                interest.scope
            );
        }
        other => panic!("expected EnsureInterest, got {other:?}"),
    }
}

fn assert_withdraw(cmd: &ActorCommand) {
    match cmd {
        ActorCommand::Interests(InterestsCommand::DropInterestOwner(identity)) => {
            assert_eq!(
                identity,
                &active_bookmark_list_identity(),
                "drop must target the active bookmark-list owner"
            );
        }
        other => panic!("expected DropInterestOwner, got {other:?}"),
    }
}
