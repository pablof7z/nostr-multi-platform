//! `ProtocolCommand` — the write-path substrate seam.
//!
//! Defined by `docs/architecture/crate-boundaries.md` §4.1.
//! NIP crates implement [`ProtocolCommand`] and dispatch through
//! [`crate::ActorCommand::Protocol`]; the actor's dispatch arm calls
//! [`ProtocolCommand::run`] with a [`ProtocolCommandContext`] that exposes
//! every operation available to the command body.
//!
//! ## Capability traits (Debt C)
//!
//! [`ProtocolCommandContextParts`] bundles six typed capability traits so the
//! constructor takes one named-field struct instead of positional args:
//! [`KernelClock`] (D7 wall-clock), [`LocalSignerAccess`] (local key +
//! active-pubkey), [`DmInboxLookup`] (kind:10050 relays), [`ErrorSurface`]
//! (D6 toast/failure), [`ActionStageTracker`] (`Requested` stage write),
//! [`RecipientRelayLookup`] (V-07 NIP-65 outbox via the kernel router).
//! D11: one public production constructor ([`ProtocolCommandContext::new`]);
//! the test-only `with_send_only` ctor is gated behind
//! `cfg(any(test, feature = "test-support"))`.
//!
//! ## Why not expose `ActorContext`
//!
//! [`crate::actor::dispatch::ActorContext`] is `pub(super)` — it would leak
//! ~18 kernel-internal fields. [`ProtocolCommandContext`] is the narrow public
//! surface: NIP crates never name `Kernel` / `IdentityRuntime` / `ActorContext`.
//!
//! ## D15 catch_unwind discipline
//!
//! Every capability accessor is wrapped in [`std::panic::catch_unwind`] so a
//! panicking host adapter cannot unwind the calling `run` frame. Read accessors
//! fall back to safe defaults (empty `Vec`, `None`, 0);
//! [`send`](ProtocolCommandContext::send)'s drop-on-panic is benign.

use std::fmt;

use crate::actor::ActorCommand;
use crate::relay::OutboundMessage;

#[path = "protocol/command_error.rs"]
mod command_error;
pub use command_error::ProtocolCommandError;

// Capability traits (Debt C) + their `Noop*` impls live in a sibling module
// (file-size discipline) and are re-exported below so the
// `crate::substrate::*` public paths are unchanged.
#[path = "protocol/capabilities.rs"]
mod capabilities;
pub use capabilities::{
    ActionStageTracker, DmInboxLookup, ErrorSurface, HostOpHandlerAccess, KernelClock,
    LocalSignerAccess, NoopActionStageTracker, NoopErrorSurface, NoopHostOpHandlerAccess,
    NoopKernelClock, NoopLocalSignerAccess, NoopRecipientRelayLookup, NoopWalletKernelAccess,
    NoopZapProfileLookup, RecipientRelayLookup, WalletKernelAccess, ZapProfileLookup,
};

// ──────────────────────────────────────────────────────────────────────────
// ProtocolCommandContext
// ──────────────────────────────────────────────────────────────────────────

/// Named-field construction recipe for [`ProtocolCommandContext`]. The
/// previous 8-positional-arg `new()` (with `#[allow(clippy::too_many_arguments)]`)
/// was collapsed onto this struct so every call site reads top-to-bottom
/// as a fully-named bundle of capability references + actor sinks.
///
/// D11 holds: this is the only public production door into the context.
/// The test-only [`ProtocolCommandContext::with_send_only`] constructor
/// is gated behind `cfg(any(test, feature = "test-support"))`.
pub struct ProtocolCommandContextParts<'a> {
    /// Re-enter the actor loop. Called from [`ProtocolCommandContext::send`].
    pub send: &'a dyn Fn(ActorCommand),
    /// Owned sender clone for handing to a spawned worker (ADR-0050 §D3a).
    pub command_sender: crate::actor::CommandSender,
    pub clock: &'a dyn KernelClock,
    pub signers: &'a dyn LocalSignerAccess,
    pub dms: &'a dyn DmInboxLookup,
    pub errors: &'a dyn ErrorSurface,
    pub stages: &'a dyn ActionStageTracker,
    pub recipients: &'a dyn RecipientRelayLookup,
    /// ADR-0052 §D4 host-op handler; noop singleton for all other commands.
    pub host_op_handler: &'a dyn HostOpHandlerAccess,
    /// ADR-0052 §D5 narrow wallet-mutation surface (NIP-47 only).
    pub wallet_kernel: &'a dyn WalletKernelAccess,
    /// ADR-0052 §D5 zap-only cached-profile read (NIP-57 only).
    pub zap_profiles: &'a dyn ZapProfileLookup,
}

/// Per-command runtime affordances handed to [`ProtocolCommand::run`].
///
/// Exposes 6 typed capability traits ([`KernelClock`], [`LocalSignerAccess`],
/// [`DmInboxLookup`], [`ErrorSurface`], [`ActionStageTracker`],
/// [`RecipientRelayLookup`]) plus 2 channel sinks ([`send`](Self::send) and
/// [`command_sender_clone`](Self::command_sender_clone)). Construction
/// goes through a single named-field [`ProtocolCommandContextParts`]
/// literal (the 12-arg closure bundle / 8-arg positional `new` are gone).
///
/// NIP crates never name `Kernel` / `IdentityRuntime` / `OutboxRouter` /
/// `MailboxCache` directly — every operation goes through this context.
pub struct ProtocolCommandContext<'a> {
    send: &'a dyn Fn(ActorCommand),
    /// Owned [`CommandSender`](crate::actor::CommandSender) clone for handing
    /// to a spawned worker thread; the test-only `with_send_only` ctor installs
    /// a sender whose receiver is dropped (sends become benign no-ops,
    /// matching D6).
    command_sender: crate::actor::CommandSender,
    clock: &'a dyn KernelClock,
    signers: &'a dyn LocalSignerAccess,
    dms: &'a dyn DmInboxLookup,
    errors: &'a dyn ErrorSurface,
    stages: &'a dyn ActionStageTracker,
    recipients: &'a dyn RecipientRelayLookup,
    /// ADR-0052 §D4 — per-app host-op handler accessor.
    host_op_handler: &'a dyn HostOpHandlerAccess,
    /// ADR-0052 §D5 — narrow wallet kernel-mutation surface (replaced the
    /// deleted `kernel: Option<&mut Kernel>` escape hatch).
    wallet_kernel: &'a dyn WalletKernelAccess,
    /// ADR-0052 §D5 — zap-only cached-profile read (replaced the generic
    /// `lnurl_for_pubkey`).
    zap_profiles: &'a dyn ZapProfileLookup,
    /// V-38: outbound-frame sink. The wallet runtime returns
    /// `Vec<OutboundMessage>` per command; the command body pushes them
    /// here so the actor's dispatch arm picks them up and routes through
    /// the existing relay-worker plumbing without re-entering through
    /// `send` (which would defer by at least one tick).
    outbound: Option<&'a mut Vec<OutboundMessage>>,
}

impl<'a> ProtocolCommandContext<'a> {
    /// Construct from a [`ProtocolCommandContextParts`] bundle (the sole
    /// public production door). Capability references close over the
    /// dispatch arm's stack-bound borrows of kernel + identity runtime;
    /// the resulting context's lifetime is the dispatch arm's stack frame.
    ///
    /// V-38: `outbound` starts as `None`; attach it via
    /// [`with_outbound`](Self::with_outbound) from the dispatch arm. ADR-0052
    /// §D5: the kernel handle is gone — wallet/zap commands reach their narrow
    /// kernel surface through the `wallet_kernel` / `zap_profiles` capabilities.
    pub fn new(parts: ProtocolCommandContextParts<'a>) -> Self {
        let ProtocolCommandContextParts {
            send,
            command_sender,
            clock,
            signers,
            dms,
            errors,
            stages,
            recipients,
            host_op_handler,
            wallet_kernel,
            zap_profiles,
        } = parts;
        Self {
            send,
            command_sender,
            clock,
            signers,
            dms,
            errors,
            stages,
            recipients,
            host_op_handler,
            wallet_kernel,
            zap_profiles,
            outbound: None,
        }
    }

    /// V-38 builder: attach an outbound-frame sink so the command body can
    /// surface relay frames produced synchronously on the actor thread.
    #[must_use]
    pub fn with_outbound(mut self, outbound: &'a mut Vec<OutboundMessage>) -> Self {
        self.outbound = Some(outbound);
        self
    }

    /// Test-only constructor that wires only the [`send`](Self::send)
    /// closure. All capability accessors return harmless defaults (0,
    /// `None`, no-op) via the noop singletons; `command_sender_clone`
    /// returns a sender whose receiver is dropped (sends become benign
    /// no-ops, matching the D6 "disconnected actor" pattern). Tests
    /// needing a specific capability build a small local adapter and
    /// pass it through [`Self::new`] via a [`ProtocolCommandContextParts`]
    /// literal.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_send_only(send: &'a dyn Fn(ActorCommand)) -> Self {
        static CLOCK: NoopKernelClock = NoopKernelClock;
        static SIGNERS: NoopLocalSignerAccess = NoopLocalSignerAccess;
        static DMS: crate::substrate::EmptyDmInboxRelayLookup =
            crate::substrate::EmptyDmInboxRelayLookup;
        static ERRORS: NoopErrorSurface = NoopErrorSurface;
        static STAGES: NoopActionStageTracker = NoopActionStageTracker;
        static RECIPIENTS: NoopRecipientRelayLookup = NoopRecipientRelayLookup;
        static HOST_OP: NoopHostOpHandlerAccess = NoopHostOpHandlerAccess;
        static WALLET: NoopWalletKernelAccess = NoopWalletKernelAccess;
        static ZAP: NoopZapProfileLookup = NoopZapProfileLookup;
        let (command_sender, _rx) = std::sync::mpsc::channel::<crate::actor::ActorMail>();
        let command_sender = crate::actor::CommandSender::new(command_sender);
        Self::new(ProtocolCommandContextParts {
            send,
            command_sender,
            clock: &CLOCK,
            signers: &SIGNERS,
            dms: &DMS,
            errors: &ERRORS,
            stages: &STAGES,
            recipients: &RECIPIENTS,
            host_op_handler: &HOST_OP,
            wallet_kernel: &WALLET,
            zap_profiles: &ZAP,
        })
    }

    /// Return an owned [`CommandSender`](crate::actor::CommandSender) clone for
    /// handing to a spawned worker thread that posts follow-up `ActorCommand`s
    /// back into the actor loop after the dispatch arm (and therefore this
    /// `ProtocolCommandContext`) has returned — the LNURL fetcher pattern
    /// (`nmp_nip57::lnurl::FetchLnurlInvoiceCommand`). The test-only
    /// `with_send_only` ctor installs a sender whose receiver is dropped
    /// (sends become benign no-ops, matching D6).
    #[must_use]
    pub fn command_sender_clone(&self) -> crate::actor::CommandSender {
        self.command_sender.clone()
    }

    /// Re-enter the actor loop with `cmd`. D15: the host-supplied closure
    /// is wrapped in [`std::panic::catch_unwind`] so a panicking follow-up
    /// cannot unwind the calling `ProtocolCommand::run` frame.
    pub fn send(&self, cmd: ActorCommand) {
        let send = self.send;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| send(cmd)));
    }

    // Extended context ports live in a child module to keep this public seam readable.
}

#[path = "protocol/context_ports.rs"]
mod context_ports;

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

#[path = "protocol/builders.rs"]
mod builders;
pub use builders::{
    build_nip44_decrypt_for_account, build_nip44_encrypt_for_account, build_record_action_failure,
    build_record_action_success, build_sign_event_for_account,
};

#[cfg(test)]
#[path = "protocol/tests.rs"]
mod tests;
