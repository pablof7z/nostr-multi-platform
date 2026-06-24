//! `FetchLnurlInvoiceCommand::run` — sign-path branches (V-78 reconcile).
//!
//! `run` emits `SignEventForAccount` (the port); its continuation spawns the
//! HTTP worker. Driving with `Ok(signed)` spawns the worker; `Err` fails
//! closed. Backend-transparent resolution is proven in nmp-core.

use super::*;
use nmp_core::actor::SignCommand;
use std::sync::Mutex;

/// Drive `run()` with a captured send sink + recordable stage tracker.
struct Sink {
    sends: Mutex<Vec<ActorCommand>>,
    stages: Mutex<Vec<String>>,
}

impl Sink {
    fn new() -> Self {
        Self {
            sends: Mutex::new(Vec::new()),
            stages: Mutex::new(Vec::new()),
        }
    }
}

/// Valid lightning address whose HTTP leg will fail (domain nonexistent) so the
/// sign-path tests stay hermetic while passing the `inject_lnurl_tag` gate.
const UNREACHABLE_LNURL: &str = "zap-test@unreachable.invalid";

struct PortCapture {
    signer_pubkey: Option<String>,
    unsigned: UnsignedEvent,
    continuation: nmp_core::SignContinuation,
    stages: Vec<String>,
    worker_rx: std::sync::mpsc::Receiver<nmp_core::ActorMail>,
}

fn run_and_capture_port(correlation_id: Option<String>) -> PortCapture {
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
    let stages = RecordingStages(Mutex::new(Vec::new()));
    let recipients = NoopRecipientRelayLookup;
    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    let signers = LocalSigner::none();
    {
        let mut ctx = ctx_with_sender(&send, nmp_core::CommandSender::new(worker_tx), &clock, &signers, &stages, &recipients);
        let cmd = Box::new(FetchLnurlInvoiceCommand {
            unsigned: unsigned_for(vec![vec!["p".to_string(), RECIPIENT_HEX.to_string()]]),
            recipient_pubkey: RECIPIENT_HEX.to_string(),
            lnurl_or_address: Some(UNREACHABLE_LNURL.to_string()),
            amount_msats: 21_000,
            correlation_id,
            // ADR-0052 rung 5.2: this test exercises the sign/LNURL legs, not
            // the wallet handoff — no payment port wired.
            payment_port: None,
        });
        cmd.run(&mut ctx).expect("run returns Ok");
    }
    let mut sends = sink.sends.into_inner().unwrap();
    assert_eq!(sends.len(), 1, "run must emit exactly one SignEventForAccount: {sends:?}");
    let (signer_pubkey, unsigned, continuation) = match sends.remove(0) {
        ActorCommand::Sign(SignCommand::EventForAccount { signer_pubkey, unsigned, continuation }) => (signer_pubkey, unsigned, continuation),
        other => panic!("expected SignEventForAccount, got {other:?}"),
    };
    PortCapture { signer_pubkey, unsigned, continuation, stages: stages.0.into_inner().unwrap(), worker_rx }
}

/// `run` must sign the kind:9734 through the unified `SignEventForAccount`
/// port with the ACTIVE account (`signer_pubkey: None`), AFTER recording the
/// Requested stage. The unsigned event carried into the port is the kind:9734
/// with `created_at` re-stamped from the context clock (D7).
#[test]
fn run_emits_sign_event_for_account_port_with_active_account() {
    let cap = run_and_capture_port(Some("cid-port".to_string()));
    assert_eq!(
        cap.signer_pubkey, None,
        "the zap request signs with the ACTIVE account (signer_pubkey = None)"
    );
    assert_eq!(cap.unsigned.kind, 9734, "the port carries the kind:9734 zap request");
    assert_eq!(
        cap.unsigned.created_at, 1_700_000_000,
        "created_at must be re-stamped from the context clock (D7)"
    );
    assert_eq!(
        cap.stages,
        vec!["cid-port".to_string()],
        "Requested stage must record once before the sign port command"
    );
}

/// V-78 reconcile — the genuine no-account / sign-error case still fails
/// closed: driving the port continuation with `Err(reason)` emits the toast +
/// `RecordActionFailure` through the worker channel. (In production the
/// dispatch arm produces this `Err` when there is no active account.)
#[test]
fn continuation_err_fails_closed_with_toast_and_failure() {
    let cap = run_and_capture_port(Some("cid-none".to_string()));
    cap.continuation
        .call(Err("no active account — sign in first".to_string()));

    let sends: Vec<ActorCommand> = cap.worker_rx.try_iter().map(nmp_core_unwrap_mail).collect();
    assert_eq!(
        sends.len(),
        2,
        "expected ShowToast + RecordActionFailure: {sends:?}"
    );
    match &sends[0] {
        ActorCommand::ShowToast { message } => {
            assert!(
                message.to_lowercase().contains("zap failed"),
                "toast must surface the zap failure: {message}"
            );
        }
        other => panic!("expected ShowToast, got {other:?}"),
    }
    match &sends[1] {
        ActorCommand::ActionLedger(ActionLedgerCommand::RecordFailure { correlation_id, .. }) => {
            assert_eq!(correlation_id, "cid-none");
        }
        other => panic!("expected RecordActionFailure, got {other:?}"),
    }
}

/// Without a `correlation_id`, a sign-error continuation emits ONLY the toast
/// (no `RecordActionFailure` — there is no spinner to clear).
#[test]
fn continuation_err_without_correlation_emits_only_toast() {
    let cap = run_and_capture_port(None);
    assert!(cap.stages.is_empty(), "no correlation_id → no Requested stage");
    cap.continuation.call(Err("no active account".to_string()));

    let sends: Vec<ActorCommand> = cap.worker_rx.try_iter().map(nmp_core_unwrap_mail).collect();
    assert_eq!(sends.len(), 1, "expected only ShowToast: {sends:?}");
    assert!(matches!(&sends[0], ActorCommand::ShowToast { .. }));
}

/// V-78 reconcile — the core proof. Whether the active account is a local
/// nsec or a NIP-46 bunker, the dispatch arm resolves the port to the SAME
/// `Ok(signed)` (proven backend-transparent in `nmp-core`). Driving this
/// command's continuation with that `Ok(signed)` spawns the off-actor HTTP
/// worker — NO synchronous fail-closed toast/failure. The worker fails at the
/// parse-failing LNURL leg (off-thread), surfacing a `ShowToast` +
/// `RecordActionFailure` through the worker channel, NOT a "no local keys"
/// rejection. This is the bunker zap working through the unified seam.
#[test]
fn continuation_ok_spawns_worker_carrying_signed_event() {
    let cap = run_and_capture_port(Some("cid-ok".to_string()));
    // Mint the SignedEvent the dispatch arm's port hands back for this
    // kind:9734 — identical shape for local nsec and bunker (the backend is
    // invisible past the port).
    let keys = Keys::generate();
    let signed = signed_for(&keys, &cap.unsigned);
    cap.continuation.call(Ok(signed));

    // The continuation spawned the HTTP worker; it fails at the unreachable
    // LNURL parse and posts its terminal through the worker channel. Block
    // briefly on the worker's first message so the off-thread send is observed
    // deterministically (no polling).
    let first = nmp_core_unwrap_mail(
        cap.worker_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("worker must post a terminal after the LNURL leg fails"),
    );
    match first {
        ActorCommand::ShowToast { message } => {
            assert!(
                message.to_lowercase().contains("zap failed"),
                "worker surfaces the LNURL failure (NOT a no-local-keys rejection): {message}"
            );
        }
        other => panic!("expected ShowToast from the worker, got {other:?}"),
    }
    // The matching RecordActionFailure follows (correlation present).
    let second = nmp_core_unwrap_mail(
        cap.worker_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("RecordActionFailure must follow when correlation_id is present"),
    );
    match second {
        ActorCommand::ActionLedger(ActionLedgerCommand::RecordFailure { correlation_id, .. }) => {
            assert_eq!(correlation_id, "cid-ok");
        }
        other => panic!("expected RecordActionFailure, got {other:?}"),
    }
}

#[test]
fn run_restamps_created_at_from_context_clock() {
    // Indirect: we can't observe `unsigned.created_at` after the move,
    // but we can verify the dispatch arm calls `now_secs` once when the
    // sentinel is `0`. Wire a counter through a custom clock adapter.
    use std::sync::atomic::{AtomicU64, Ordering};

    struct CountingClock(AtomicU64);
    impl KernelClock for CountingClock {
        fn now_secs(&self) -> u64 {
            self.0.fetch_add(1, Ordering::SeqCst);
            1_700_000_000
        }
    }

    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = CountingClock(AtomicU64::new(0));
    let signers = LocalSigner::none();
    let stages = RecordingStages(Mutex::new(Vec::new()));
    let recipients = NoopRecipientRelayLookup;
    let mut ctx = ctx_with(&send, &clock, &signers, &stages, &recipients);

    let cmd = Box::new(FetchLnurlInvoiceCommand {
        unsigned: unsigned_for(vec![
            vec!["p".to_string(), RECIPIENT_HEX.to_string()],
        ]),
        recipient_pubkey: RECIPIENT_HEX.to_string(),
        lnurl_or_address: Some("alice@example.com".to_string()),
        amount_msats: 21_000,
        correlation_id: None,
        // ADR-0052 rung 5.2: sign/LNURL-leg test, no payment port wired.
        payment_port: None,
    });
    cmd.run(&mut ctx).expect("run returns Ok on fail-closed branch");
    assert!(
        clock.0.load(Ordering::SeqCst) >= 1,
        "now_secs must be invoked when created_at sentinel is 0"
    );
}
