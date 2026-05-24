//! `ProtocolCommand` — the write-path substrate seam.
//!
//! Defined by `docs/architecture/crate-boundaries.md` §4.1. Step 1.b of the
//! 12-step migration: pure addition + one new [`crate::ActorCommand`] variant
//! (`Protocol(Box<dyn ProtocolCommand>)`). No NIP code moves yet — the
//! existing closed-enum arms keep working.
//!
//! The seam exists so NIP crates (NIP-17 DM send, NIP-47 NWC pay, NIP-57
//! LNURL fetch, …) stop adding bespoke variants to [`crate::ActorCommand`].
//! They instead dispatch `ActorCommand::Protocol(Box::new(MyCommand{...}))`
//! and the kernel calls `cmd.run(&mut ctx)`. Steps 4–7 of the migration move
//! `FetchLnurlInvoice` (V-41), `SendGiftWrappedDm` (V-39), and the three
//! `Wallet*` variants (V-38) onto this trait.
//!
//! ## Why a wrapper context type (`ProtocolCommandContext`) and not `ActorContext`
//!
//! The spec's §4.1 sketches `&mut ActorContext` as the context arg.
//! [`crate::actor::dispatch::ActorContext`] is intentionally `pub(super)` —
//! exposing it would publish 18 fields' worth of kernel internals to every
//! NIP crate. Instead the dispatch arm constructs a public
//! [`ProtocolCommandContext`] that exposes only what the trait needs.
//! Step 1.b ships the minimal surface: a self-send closure for follow-up
//! [`ActorCommand`]s. Step 4 (V-41) expands it with signer access; step 5
//! (V-39) adds `SignerForSealCapability`. Expanding a struct method by method
//! is forward-compatible; un-exposing a leaked field is not.

use std::fmt;

use crate::kernel::Kernel;
use crate::relay::OutboundMessage;
use crate::ActorCommand;

/// Error returned by a [`ProtocolCommand::run`]. Kernel surfaces it as the
/// `last_error_toast` projection (step 4+); step 1.b just logs.
#[derive(Debug)]
pub struct ProtocolCommandError {
    message: String,
}

impl ProtocolCommandError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ProtocolCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ProtocolCommandError {}

/// Per-command runtime affordances handed to [`ProtocolCommand::run`].
///
/// Step 1.b surface: a self-send closure that re-enters the actor loop with a
/// follow-up [`ActorCommand`] (the LNURL fetcher pattern — kick off a
/// worker, have the worker feed bolt11 back as a follow-up command, never
/// block the actor). Later migrations widen this with signer access, store
/// lookup hooks, and projection writers as those needs land.
pub struct ProtocolCommandContext<'a> {
    send: &'a dyn Fn(ActorCommand),
    /// V-38: optional `&mut Kernel` for command bodies that need to mutate
    /// kernel state synchronously on the actor thread — record action
    /// terminals, set the last-error toast, register persistent subs, mark
    /// the snapshot dirty. `None` only in the substrate's own unit tests
    /// that construct a context without a kernel; production dispatch
    /// always sets it.
    kernel: Option<&'a mut Kernel>,
    /// V-38: outbound-frame sink. The wallet runtime returns
    /// `Vec<OutboundMessage>` per command; the command body pushes them
    /// here so the actor's dispatch arm picks them up and routes through
    /// the existing relay-worker plumbing without re-entering through
    /// `send` (which would defer by at least one tick).
    outbound: Option<&'a mut Vec<OutboundMessage>>,
}

impl<'a> ProtocolCommandContext<'a> {
    /// Constructed by the kernel dispatch arm. Test code constructs it with
    /// a closure that captures whatever recording state the test needs.
    pub fn new(send: &'a dyn Fn(ActorCommand)) -> Self {
        Self {
            send,
            kernel: None,
            outbound: None,
        }
    }

    /// Builder: attach the actor's kernel handle. The dispatch arm calls
    /// this before invoking [`ProtocolCommand::run`].
    #[must_use]
    pub fn with_kernel(mut self, kernel: &'a mut Kernel) -> Self {
        self.kernel = Some(kernel);
        self
    }

    /// Builder: attach an outbound-frame sink so the command body can
    /// surface relay frames produced synchronously on the actor thread.
    #[must_use]
    pub fn with_outbound(mut self, outbound: &'a mut Vec<OutboundMessage>) -> Self {
        self.outbound = Some(outbound);
        self
    }

    /// Re-enter the actor loop with `cmd`. The actor processes it in a
    /// subsequent dispatch cycle (same thread, same channel).
    ///
    /// D15: the closure is host-supplied (constructed by the dispatch
    /// arm but conceptually owned by the kernel boundary), so the
    /// invocation is wrapped in [`catch_unwind`] to keep a panicking
    /// follow-up from unwinding the calling `ProtocolCommand`'s
    /// `run()` frame.
    pub fn send(&self, cmd: ActorCommand) {
        let send = self.send;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| send(cmd)));
    }

    /// Reborrow the actor's kernel handle. `None` only in unit tests that
    /// constructed the context without one.
    pub fn kernel_mut(&mut self) -> Option<&mut Kernel> {
        self.kernel.as_deref_mut()
    }

    /// Push outbound relay frames produced synchronously by the command
    /// body. The actor's dispatch arm drains them into the existing
    /// `send_all_outbound` plumbing. No-op when no outbound sink is attached
    /// (unit tests).
    pub fn push_outbound<I: IntoIterator<Item = OutboundMessage>>(&mut self, frames: I) {
        if let Some(out) = self.outbound.as_mut() {
            out.extend(frames);
        }
    }
}

/// Open-seam command dispatched as [`ActorCommand::Protocol`].
///
/// `Debug` is required because [`ActorCommand`] derives `Debug` and the
/// boxed variant transitively forwards to the trait object. The default
/// derive on a NIP crate's struct is normally sufficient.
pub trait ProtocolCommand: Send + fmt::Debug + 'static {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError>;
}

#[cfg(test)]
mod tests {
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
        let cmd: Box<dyn ProtocolCommand> =
            Box::new(FiringCommand { fired: fired.clone() });

        let send = |_: ActorCommand| {};
        let mut ctx = ProtocolCommandContext::new(&send);
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
        let mut ctx = ProtocolCommandContext::new(&send);

        let cmd: Box<dyn ProtocolCommand> = Box::new(ChainingCommand);
        cmd.run(&mut ctx).expect("ChainingCommand returns Ok");

        let recorded = sent.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert!(recorded[0].contains("Shutdown"), "got: {}", recorded[0]);
    }

    #[test]
    fn run_propagates_error() {
        let send = |_: ActorCommand| {};
        let mut ctx = ProtocolCommandContext::new(&send);
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
}
