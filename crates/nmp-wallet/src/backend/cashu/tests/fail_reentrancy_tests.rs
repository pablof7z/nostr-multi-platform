//! #2953 regression — the `fail(..)` helpers in `send.rs`/`redeem.rs` re-lock
//! the backend's non-reentrant `std::sync::Mutex` (via `lock_state`). Both the
//! `SendNutzapCommand::run` and `RedeemNutzapCommand::run` `JOURNAL_ERROR`
//! branches invoke `fail(..)` from inside a `return EXPR;` while a NAMED
//! `MutexGuard` (`let mut s = lock_state(&state);`) is still in scope for the
//! enclosing block. `return EXPR;` evaluates `EXPR` BEFORE unwinding that block
//! and dropping `s`, so without an explicit `drop(s)` first the thread
//! re-locks a mutex it already holds and hangs forever.
//!
//! This test drives the SEND path's branch deterministically (no live mint,
//! no DLEQ round-trip) and asserts `run()` COMPLETES rather than hangs, using a
//! watchdog thread + bounded `recv_timeout` so the pre-fix deadlock fails the
//! test fast in CI instead of wedging the whole suite. The redeem path's own
//! `JOURNAL_ERROR` branch is the identical code shape with the identical fix,
//! but is gated behind `verify_nutzap_dleq`'s live-mint HTTP round-trip (a
//! genuine DLEQ can only be produced with `nmp-nip60`'s `#[cfg(test)]`-only
//! signing helpers, unreachable from this crate) — so this deterministic
//! send-path test is the regression guard for the shared hazard fixed in both
//! files.
//!
//! Reaching the `JOURNAL_ERROR` branch needs `s.transition(.., MintPending)` to
//! return `Err`. The realistic production trigger is a concurrent
//! `CashuWalletBackend::reset()` (active-account switch mid-flight) wiping the
//! operation out from under an in-flight `run()`, making the later transition
//! return `MissingOperation`. This test reproduces the SAME `Err`-from-transition
//! condition deterministically by pre-moving the operation to the terminal
//! `Failed` state (from which `MintPending` is an invalid transition) after
//! `start_intent` has created it but before `run()` reaches the transition.

use super::*;

fn ctx(account_pubkey: Option<&str>) -> WalletBackendContext<'_> {
    WalletBackendContext {
        now_secs: 1_700_000_000,
        selected_backend: None,
        account_pubkey,
    }
}

// Each `tests/` submodule defines its own small fixtures rather than sharing
// across siblings (matches `send_tests.rs`/`redeem_tests.rs`).
const ACCOUNT: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const RECIPIENT: &str = "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";

#[test]
fn send_journal_error_branch_does_not_self_deadlock_on_a_held_guard() {
    let backend = backend_with_mint();

    // Enough inventory at the accepted mint that `select_proofs` succeeds and
    // `run()` gets all the way to the `MintPending` transition block.
    {
        let mut s = state::lock_state(&backend.state);
        s.add_proofs(
            Some("token-event-1".to_string()),
            MINT.to_string(),
            vec![synthetic_proof(30, "02aa")],
        );
    }

    // A recipient whose kind:10019 clears every earlier gate (relays present,
    // a mutually-trusted mint, a Cashu P2PK pubkey) so the only failure left is
    // the engineered transition error.
    let info = nmp_nip60::nutzap::NutZapInfo {
        relays: vec!["wss://recipient-relay.example".to_string()],
        mints: vec![MINT.to_string()],
        cashu_pubkey: Some("02".to_string() + &"44".repeat(32)),
    };
    let recipient_event = nutzap_info_kernel_event(RECIPIENT, &info, 1_699_999_000);

    let correlation_id = "cid-2953-send-deadlock";
    let commands = backend.start_intent(
        ctx(Some(ACCOUNT)),
        WalletIntent::SendNutzap {
            recipient_pubkey: RECIPIENT.to_string(),
            amount_sats: 21,
            target_event_id: None,
        },
        Some(correlation_id.to_string()),
    );
    let ActorCommand::Protocol(cmd) = commands.into_iter().next().unwrap() else {
        panic!("expected a Protocol command");
    };

    // Stand in for the concurrent `reset()` race: move the just-created
    // operation to a terminal state so `run()`'s own `Prepared -> MintPending`
    // becomes an invalid transition — the same `Err`-from-`transition` the real
    // `MissingOperation` race produces, and the exact condition that drives the
    // `JOURNAL_ERROR` branch this bug lives in.
    let operation_id = crate::journal::WalletOperationId::new(correlation_id);
    {
        let mut s = state::lock_state(&backend.state);
        s.transition(&operation_id, crate::journal::WalletOperationState::Failed)
            .expect("Prepared -> Failed is a valid transition");
    }

    // Run on a watchdog thread: if the held-guard `fail(..)` re-lock regressed,
    // this thread wedges on its own mutex and `recv_timeout` fires instead of
    // the whole test binary hanging. Everything the ctx borrows is built and
    // owned INSIDE the thread; only the shared error surface (Send + Sync) is
    // handed across so the assertions below can read what `fail(..)` reported.
    let errors = std::sync::Arc::new(RecordingErrorSurface::default());
    let errors_for_thread = std::sync::Arc::clone(&errors);
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let handle = std::thread::spawn(move || {
        let sink = Sink::new();
        let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
        let clock = FixedClock(1_700_000_000);
        let recipients = FixedRecipientLookup(vec!["wss://my-relay.example".to_string()]);
        let cached = FixedCachedEvents(vec![recipient_event]);
        let (worker_tx, _worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
        {
            let mut c = ctx_with_cached_events_and_errors(
                &send,
                nmp_core::CommandSender::new(worker_tx),
                &clock,
                &recipients,
                &cached,
                errors_for_thread.as_ref(),
            );
            cmd.run(&mut c).expect("run returns Ok even on the fail-closed branch");
        }
        let _ = done_tx.send(());
    });

    match done_rx.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(()) => handle.join().expect("run thread must not panic"),
        Err(_) => panic!(
            "SendNutzapCommand::run JOURNAL_ERROR branch self-deadlocked (#2953): \
             fail(..) re-locked a MutexGuard still held by the enclosing block"
        ),
    }

    expect_error_code(&errors, ui_codes::JOURNAL_ERROR);

    let s = state::lock_state(&backend.state);
    let op = s
        .journal
        .get(&operation_id)
        .expect("the operation must still be journaled, marked Failed");
    assert_eq!(op.state, crate::journal::WalletOperationState::Failed);
}

fn expect_error_code(errors: &RecordingErrorSurface, code: &str) {
    assert_eq!(errors.last_token_code.lock().unwrap().as_deref(), Some(code));
}
