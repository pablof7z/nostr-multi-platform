//! `ResolveNip05Command` behavior tests.
//!
//! `parse_nip05` shape cases and the `names`-map JSON parse live next to their
//! code (`parse.rs` / `http.rs`). This module covers the `ProtocolCommand`
//! integration: a failed lookup must surface a diagnostic on the actor channel,
//! never be swallowed (D6).

#![cfg(feature = "native")]

use std::sync::mpsc::Receiver;
use std::time::Duration;

use nmp_core::actor::ActionLedgerCommand;
use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{ProtocolCommand, ProtocolCommandContext};
use nmp_core::ActorMail;

use crate::ResolveNip05Command;

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
