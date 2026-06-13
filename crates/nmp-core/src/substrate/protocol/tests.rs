use super::*;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
struct FiringCommand {
    fired: Arc<Mutex<bool>>,
}

impl ProtocolCommand for FiringCommand {
    fn run(
        self: Box<Self>,
        _ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        *self.fired.lock().unwrap() = true;
        Ok(())
    }
}

#[derive(Debug)]
struct ChainingCommand;

impl ProtocolCommand for ChainingCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        ctx.send(ActorCommand::Shutdown);
        Ok(())
    }
}

#[derive(Debug)]
struct FailingCommand;

impl ProtocolCommand for FailingCommand {
    fn run(
        self: Box<Self>,
        _ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        Err(ProtocolCommandError::new("intentional"))
    }
}

#[test]
fn run_is_called_with_context() {
    let fired = Arc::new(Mutex::new(false));
    let cmd: Box<dyn ProtocolCommand> = Box::new(FiringCommand {
        fired: fired.clone(),
    });

    let send = |_: ActorCommand| {};
    let mut ctx = ProtocolCommandContext::with_send_only(&send);
    cmd.run(&mut ctx).expect("FiringCommand returns Ok");

    assert!(*fired.lock().unwrap());
}

#[test]
fn context_send_reaches_closure() {
    let sent = Arc::new(Mutex::new(Vec::<String>::new()));
    let sent_clone = sent.clone();
    let send = move |cmd: ActorCommand| {
        sent_clone.lock().unwrap().push(format!("{cmd:?}"));
    };
    let mut ctx = ProtocolCommandContext::with_send_only(&send);

    let cmd: Box<dyn ProtocolCommand> = Box::new(ChainingCommand);
    cmd.run(&mut ctx).expect("ChainingCommand returns Ok");

    let recorded = sent.lock().unwrap();
    assert_eq!(recorded.len(), 1);
    assert!(recorded[0].contains("Shutdown"), "got: {}", recorded[0]);
}

#[test]
fn run_propagates_error() {
    let send = |_: ActorCommand| {};
    let mut ctx = ProtocolCommandContext::with_send_only(&send);
    let cmd: Box<dyn ProtocolCommand> = Box::new(FailingCommand);

    let err = cmd.run(&mut ctx).expect_err("FailingCommand returns Err");
    assert_eq!(err.message(), "intentional");
}

#[test]
fn actor_command_protocol_variant_is_debug_safe() {
    // ActorCommand derives Debug; the Protocol variant must format
    // without panicking even with an opaque payload.
    let cmd = ActorCommand::Protocol(Box::new(ChainingCommand));
    let s = format!("{cmd:?}");
    assert!(s.contains("Protocol"), "got: {s}");
}

#[test]
fn with_send_only_defaults_are_safe() {
    // Debt C — `with_send_only` wires the noop capability singletons.
    // All accessors return harmless defaults; the dispatch arm does
    // not panic on any of them.
    let send = |_: ActorCommand| {};
    let ctx = ProtocolCommandContext::with_send_only(&send);
    assert_eq!(ctx.now_secs(), 0);
    assert!(ctx.active_local_keys().is_none());
    assert!(ctx.active_account_pubkey().is_none());
    assert!(ctx.dm_inbox_relays("anything").is_none());
    ctx.set_last_error_toast(Some("toast".to_string()));
    ctx.record_action_failure("cid".to_string(), "err".to_string());
    ctx.record_action_stage_requested("cid-noop");
    assert!(ctx.recipient_publish_relays("anyone", 9735).is_empty());
}

// ── Capability adapters used by the full-constructor test ──

struct FixedClock(u64);
impl KernelClock for FixedClock {
    fn now_secs(&self) -> u64 {
        self.0
    }
}

struct LocalSigners {
    keys: Option<nostr::Keys>,
    active_pubkey: Option<String>,
}
impl LocalSignerAccess for LocalSigners {
    fn active_local_keys(&self) -> Option<nostr::Keys> {
        self.keys.clone()
    }
    fn active_account_pubkey(&self) -> Option<String> {
        self.active_pubkey.clone()
    }
}

struct RecordingErrors {
    toasts: Mutex<Vec<Option<String>>>,
    failures: Mutex<Vec<(String, String)>>,
}
impl ErrorSurface for RecordingErrors {
    fn set_last_error_toast(&self, message: Option<String>) {
        self.toasts.lock().unwrap().push(message);
    }
    fn record_action_failure(&self, correlation_id: String, reason: String) {
        self.failures.lock().unwrap().push((correlation_id, reason));
    }
}

struct RecordingStages {
    seen: Mutex<Vec<String>>,
}
impl ActionStageTracker for RecordingStages {
    fn record_requested(&self, correlation_id: &str) {
        self.seen.lock().unwrap().push(correlation_id.to_string());
    }
}

struct RecordingRecipients {
    seen: Mutex<Vec<(String, u32)>>,
    respond: Vec<String>,
}
impl RecipientRelayLookup for RecordingRecipients {
    fn recipient_publish_relays(&self, recipient: &str, kind: u32) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .push((recipient.to_string(), kind));
        self.respond.clone()
    }
}

#[test]
fn full_constructor_threads_capabilities() {
    use std::sync::mpsc;
    let send = |_: ActorCommand| {};
    let clock = FixedClock(123_456);
    let signers = LocalSigners {
        keys: None,
        active_pubkey: None,
    };
    let dms = crate::substrate::EmptyDmInboxRelayLookup;
    let errors = RecordingErrors {
        toasts: Mutex::new(Vec::new()),
        failures: Mutex::new(Vec::new()),
    };
    let stages = RecordingStages {
        seen: Mutex::new(Vec::new()),
    };
    let recipients = RecordingRecipients {
        seen: Mutex::new(Vec::new()),
        respond: vec!["wss://r.example".to_string()],
    };
    let (tx, rx) = mpsc::channel::<crate::actor::ActorMail>();

    let host_op_handler = crate::substrate::protocol::NoopHostOpHandlerAccess;
    let wallet_kernel = crate::substrate::protocol::NoopWalletKernelAccess;
    let zap_profiles = crate::substrate::protocol::NoopZapProfileLookup;
    let ctx = ProtocolCommandContext::new(ProtocolCommandContextParts {
        send: &send,
        command_sender: crate::actor::CommandSender::new(tx),
        clock: &clock,
        signers: &signers,
        dms: &dms,
        errors: &errors,
        stages: &stages,
        recipients: &recipients,
        host_op_handler: &host_op_handler,
        wallet_kernel: &wallet_kernel,
        zap_profiles: &zap_profiles,
    });

    assert_eq!(ctx.now_secs(), 123_456);
    assert!(ctx.active_local_keys().is_none());
    assert!(ctx.active_account_pubkey().is_none());
    assert!(ctx.dm_inbox_relays("anyone").is_none());
    ctx.set_last_error_toast(Some("hello".to_string()));
    ctx.record_action_failure("cid-z".to_string(), "boom".to_string());
    ctx.record_action_stage_requested("cid-abc");
    assert_eq!(
        *errors.toasts.lock().unwrap(),
        vec![Some("hello".to_string())]
    );
    assert_eq!(
        *errors.failures.lock().unwrap(),
        vec![("cid-z".to_string(), "boom".to_string())]
    );
    assert_eq!(*stages.seen.lock().unwrap(), vec!["cid-abc".to_string()]);

    let urls = ctx.recipient_publish_relays("alice", 9735);
    assert_eq!(urls, vec!["wss://r.example".to_string()]);
    assert_eq!(
        *recipients.seen.lock().unwrap(),
        vec![("alice".to_string(), 9735u32)]
    );

    // Worker-side sender clone reaches the matching receiver.
    let cloned = ctx.command_sender_clone();
    cloned.send(ActorCommand::Shutdown).expect("send");
    match rx.recv().unwrap() {
        crate::actor::ActorMail::Command(ActorCommand::Shutdown) => (),
        other => panic!("expected Shutdown, got {other:?}"),
    }
}

#[test]
fn with_send_only_provides_disconnected_sender() {
    let send = |_: ActorCommand| {};
    let ctx = ProtocolCommandContext::with_send_only(&send);
    let cloned = ctx.command_sender_clone();
    assert!(cloned.send(ActorCommand::Shutdown).is_err());
}
