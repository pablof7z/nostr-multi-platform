//! `ResolveNip05Command` behavior tests.
//!
//! `parse_nip05` shape cases and the `names`-map JSON parse live next to their
//! code (`parse.rs` / `http.rs`). This module covers the `ProtocolCommand`
//! integration: a failed lookup must surface a diagnostic on the actor channel,
//! never be swallowed (D6).

#![cfg(feature = "native")]

use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use nmp_core::actor::ActionLedgerCommand;
use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{ProtocolCommand, ProtocolCommandContext};
use nmp_core::ActorMail;

use crate::{Nip05LookupObserver, ResolveNip05Command};

/// Drain the worker channel until a terminal command arrives or the budget
/// elapses. The worker is a real `std::thread` doing a (failing) HTTP attempt,
/// so the budget is generous; a `.invalid` TLD (RFC 6761) fails fast without a
/// network round-trip.
fn drain_one(rx: &Receiver<ActorMail>, budget: Duration) -> Option<ActorCommand> {
    match rx.recv_timeout(budget) {
        Ok(ActorMail::Command(cmd)) => Some(cmd),
        _ => None,
    }
}

#[test]
fn failed_lookup_emits_diagnostic_toast_and_failure_record() {
    let cmd = ResolveNip05Command {
        name: "alice".to_string(),
        domain: "nonexistent.invalid".to_string(),
        correlation_id: Some("corr-1".to_string()),
        observer: None,
    };

    let send = |_c: ActorCommand| {};
    let (tx, rx) = std::sync::mpsc::channel::<ActorMail>();
    let mut ctx = ProtocolCommandContext::new(nmp_core::substrate::ProtocolCommandContextParts {
        send: &send,
        command_sender: nmp_core::CommandSender::new(tx),
        clock: &nmp_core::substrate::NoopKernelClock,
        signers: &nmp_core::substrate::NoopLocalSignerAccess,
        dms: &nmp_core::substrate::EmptyDmInboxRelayLookup,
        errors: &nmp_core::substrate::NoopErrorSurface,
        stages: &nmp_core::substrate::NoopActionStageTracker,
        recipients: &nmp_core::substrate::NoopRecipientRelayLookup,
        host_op_handler: &nmp_core::substrate::NoopHostOpHandlerAccess,
        wallet_kernel: &nmp_core::substrate::NoopWalletKernelAccess,
        zap_profiles: &nmp_core::substrate::NoopZapProfileLookup,
    });

    Box::new(cmd)
        .run(&mut ctx)
        .expect("run returns Ok (work is deferred to the worker)");

    // The worker posts ShowErrorToken first, then RecordActionFailure. The
    // `.invalid` TLD fails DNS without a real round-trip, so a few seconds
    // is ample.
    let budget = Duration::from_secs(15);
    let first = drain_one(&rx, budget).expect("worker must emit a diagnostic, never swallow");
    let token = match first {
        ActorCommand::ShowErrorToken { token } => token,
        other => panic!("expected ShowErrorToken, got {other:?}"),
    };
    assert_eq!(
        token.code(),
        crate::ui_codes::LOOKUP_FAILED,
        "failed lookup must carry LOOKUP_FAILED code"
    );
    assert!(
        token
            .subject()
            .unwrap_or("")
            .contains("alice@nonexistent.invalid"),
        "token subject must name the identifier: {:?}",
        token.subject()
    );

    let second = drain_one(&rx, Duration::from_secs(2))
        .expect("a correlation_id must yield a RecordActionFailure");
    match second {
        ActorCommand::ActionLedger(ActionLedgerCommand::RecordFailure {
            correlation_id,
            reason: _,
        }) => {
            assert_eq!(correlation_id, "corr-1");
        }
        other => panic!("expected RecordActionFailure, got {other:?}"),
    }
}

#[test]
fn failed_lookup_without_correlation_id_still_toasts_but_records_nothing() {
    let cmd = ResolveNip05Command {
        name: "bob".to_string(),
        domain: "nonexistent.invalid".to_string(),
        correlation_id: None,
        observer: None,
    };

    let send = |_c: ActorCommand| {};
    let (tx, rx) = std::sync::mpsc::channel::<ActorMail>();
    let mut ctx = ProtocolCommandContext::new(nmp_core::substrate::ProtocolCommandContextParts {
        send: &send,
        command_sender: nmp_core::CommandSender::new(tx),
        clock: &nmp_core::substrate::NoopKernelClock,
        signers: &nmp_core::substrate::NoopLocalSignerAccess,
        dms: &nmp_core::substrate::EmptyDmInboxRelayLookup,
        errors: &nmp_core::substrate::NoopErrorSurface,
        stages: &nmp_core::substrate::NoopActionStageTracker,
        recipients: &nmp_core::substrate::NoopRecipientRelayLookup,
        host_op_handler: &nmp_core::substrate::NoopHostOpHandlerAccess,
        wallet_kernel: &nmp_core::substrate::NoopWalletKernelAccess,
        zap_profiles: &nmp_core::substrate::NoopZapProfileLookup,
    });

    Box::new(cmd).run(&mut ctx).expect("run returns Ok");

    let first = drain_one(&rx, Duration::from_secs(15)).expect("worker must emit a diagnostic");
    assert!(matches!(first, ActorCommand::ShowErrorToken { .. }));
    // No correlation id → no RecordActionFailure follows.
    assert!(
        drain_one(&rx, Duration::from_millis(200)).is_none(),
        "no correlation_id means no failure record"
    );
}

/// Test double recording a [`Nip05LookupObserver`] terminal callback onto a
/// channel — the search UI's real poll seam is a state map updated the same
/// way (chirp#155), so asserting the observer fires exactly once, with an
/// outcome, is the crate-level proof that a dispatched lookup can never spin
/// forever: it either resolves or fails within the worker's own bound.
struct RecordingObserver {
    tx: Sender<Result<String, String>>,
}

impl Nip05LookupObserver for RecordingObserver {
    fn on_resolved(&self, pubkey: &str) {
        let _ = self.tx.send(Ok(pubkey.to_string()));
    }

    fn on_failed(&self, reason: &str) {
        let _ = self.tx.send(Err(reason.to_string()));
    }
}

#[test]
fn failed_lookup_notifies_observer_within_a_bounded_time() {
    // chirp#155: `SearchView`'s "Looking up …" affordance never transitioned
    // out because nothing polled a terminal outcome for the dispatched
    // identifier. This reproduces the missing seam directly: a lookup against
    // an unreachable domain must notify the observer (never just the generic
    // `ShowErrorToken` toast) inside a bounded window, or this test times out
    // exactly the way the real UI hung.
    let (obs_tx, obs_rx) = std::sync::mpsc::channel();
    let cmd = ResolveNip05Command {
        name: "carol".to_string(),
        domain: "nonexistent.invalid".to_string(),
        correlation_id: None,
        observer: Some(Arc::new(RecordingObserver { tx: obs_tx })),
    };

    let send = |_c: ActorCommand| {};
    let (tx, _rx) = std::sync::mpsc::channel::<ActorMail>();
    let mut ctx = ProtocolCommandContext::new(nmp_core::substrate::ProtocolCommandContextParts {
        send: &send,
        command_sender: nmp_core::CommandSender::new(tx),
        clock: &nmp_core::substrate::NoopKernelClock,
        signers: &nmp_core::substrate::NoopLocalSignerAccess,
        dms: &nmp_core::substrate::EmptyDmInboxRelayLookup,
        errors: &nmp_core::substrate::NoopErrorSurface,
        stages: &nmp_core::substrate::NoopActionStageTracker,
        recipients: &nmp_core::substrate::NoopRecipientRelayLookup,
        host_op_handler: &nmp_core::substrate::NoopHostOpHandlerAccess,
        wallet_kernel: &nmp_core::substrate::NoopWalletKernelAccess,
        zap_profiles: &nmp_core::substrate::NoopZapProfileLookup,
    });

    Box::new(cmd).run(&mut ctx).expect("run returns Ok");

    let outcome = obs_rx.recv_timeout(Duration::from_secs(15)).expect(
        "observer must be notified of a terminal outcome — a search UI polling this seam \
         must never see an eternal \"Looking up …\" state (#155)",
    );
    assert!(
        matches!(outcome, Err(_)),
        "nonexistent.invalid must terminate as a failure, not a resolve"
    );
}

#[test]
fn invalid_shape_notifies_observer_synchronously() {
    // The re-validation short-circuit runs on the calling thread (no worker
    // spawned), so this proves the observer fires even on that early-return
    // path — no channel/timeout needed, `run` returning is enough.
    let (obs_tx, obs_rx) = std::sync::mpsc::channel();
    let cmd = ResolveNip05Command {
        name: "dave".to_string(),
        // An empty domain fails `parse_nip05`'s re-validation.
        domain: String::new(),
        correlation_id: None,
        observer: Some(Arc::new(RecordingObserver { tx: obs_tx })),
    };

    let send = |_c: ActorCommand| {};
    let (tx, _rx) = std::sync::mpsc::channel::<ActorMail>();
    let mut ctx = ProtocolCommandContext::new(nmp_core::substrate::ProtocolCommandContextParts {
        send: &send,
        command_sender: nmp_core::CommandSender::new(tx),
        clock: &nmp_core::substrate::NoopKernelClock,
        signers: &nmp_core::substrate::NoopLocalSignerAccess,
        dms: &nmp_core::substrate::EmptyDmInboxRelayLookup,
        errors: &nmp_core::substrate::NoopErrorSurface,
        stages: &nmp_core::substrate::NoopActionStageTracker,
        recipients: &nmp_core::substrate::NoopRecipientRelayLookup,
        host_op_handler: &nmp_core::substrate::NoopHostOpHandlerAccess,
        wallet_kernel: &nmp_core::substrate::NoopWalletKernelAccess,
        zap_profiles: &nmp_core::substrate::NoopZapProfileLookup,
    });

    Box::new(cmd).run(&mut ctx).expect("run returns Ok");

    let outcome = obs_rx
        .try_recv()
        .expect("invalid shape must notify the observer before `run` returns");
    assert!(matches!(outcome, Err(_)), "empty domain is not valid NIP-05 shape");
}
