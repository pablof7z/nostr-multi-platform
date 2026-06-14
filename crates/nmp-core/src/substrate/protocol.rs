//! `ProtocolCommand` — the write-path substrate seam.
//!
//! Defined by `docs/architecture/crate-boundaries.md` §4.1. Step 1.b of the
//! 12-step migration: pure addition + one new [`crate::ActorCommand`] variant
//! (`Protocol(Box<dyn ProtocolCommand>)`). Step 4 (V-41) added the kernel +
//! identity accessors the NIP-57 LNURL fetcher needs; V-39+V-40 (NIP-17 DM
//! stack) added the local-keys snapshot, DM-inbox relay lookup, and D6 error
//! surface; ADR-0050 §D5 replaced the `SignerForSeal` resolver with
//! `active_account_pubkey` (the gift-wrap chain signs through the actor port).
//!
//! ## Debt C — capability traits replace a 12-arg closure bundle
//!
//! Pre-Debt C the dispatch arm threaded 12 individual closures into
//! [`ProtocolCommandContext::new`]. The follow-up (V-41 + V-39+V-40 + V-08
//! bunker DM) reduced it to 6 typed capability traits plus 2 channel sinks
//! (`send`, `command_sender`), then a collapse pass folded those 8 positional
//! args into one named-field [`ProtocolCommandContextParts`] struct so the
//! constructor takes one arg. D11 still holds: one public production
//! constructor, [`ProtocolCommandContext::new`]; the test-only
//! [`ProtocolCommandContext::with_send_only`] is gated behind
//! `cfg(any(test, feature = "test-support"))`.
//!
//! Capability traits bundled by the parts struct:
//!
//! - [`KernelClock`] — D7 wall-clock seam.
//! - [`LocalSignerAccess`] — local `nostr::Keys` snapshot + backend-transparent
//!   `active_account_pubkey` (the gift-wrap chain's account-pinning source).
//! - [`DmInboxLookup`] — kind:10050 DM-inbox relay reads (concrete cache
//!   lives in `nmp-nip17`).
//! - [`ErrorSurface`] — D6 `last_error_toast` + `Failed` action-stage
//!   recorder. Fired on every early-exit branch.
//! - [`ActionStageTracker`] — `Requested` stage write.
//! - [`RecipientRelayLookup`] — V-07 NIP-57 LNURL `relays` tag injection;
//!   kernel adapter wraps `outbox_router.route_publish` with a synthetic
//!   publish-direction `UnsignedEvent` (recipient NIP-65 write set, with
//!   router lane-7/lane-6 cold-start fallback).
//!
//! NIP commands call `ctx.clock().now_secs()`, `ctx.signers().active_account_pubkey()`,
//! `ctx.dms().dm_inbox_relays(pk)`, `ctx.recipients().recipient_publish_relays(pk, kind)`,
//! etc. — trait names tell every reader which surface a given call belongs to.
//!
//! Routing accessors (`author_write_relays`, `bootstrap_discovery_relays`)
//! were removed in the Debt-A overlap: NIP commands that need a recipient
//! relay set MUST go through `RecipientRelayLookup` (which drives the
//! kernel's `OutboxRouter`).
//!
//! ## Why a wrapper context (`ProtocolCommandContext`) and not `ActorContext`
//!
//! [`crate::actor::dispatch::ActorContext`] is intentionally `pub(super)` —
//! exposing it would publish ~18 fields of kernel internals to every NIP
//! crate. Instead the dispatch arm constructs a public
//! [`ProtocolCommandContext`] that exposes only what the trait needs.
//! NIP crates never name `Kernel` / `IdentityRuntime` / `ActorContext` —
//! every operation a `ProtocolCommand::run` body can perform is a method
//! on `ProtocolCommandContext`.
//!
//! ## D15 catch_unwind discipline
//!
//! Every accessor that fires a capability method is wrapped in
//! [`std::panic::catch_unwind`] so a panicking host-side adapter cannot
//! unwind the calling `ProtocolCommand::run` frame. Read accessors fall
//! back to safe defaults on panic (empty `Vec`, `None`, 0);
//! [`send`](ProtocolCommandContext::send)'s drop-on-panic is benign.

use std::fmt;

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
        Self {
            message: message.into(),
        }
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
    /// Owned actor-command sender clone the command's `run` body can hand
    /// to a spawned worker thread (the LNURL fetcher pattern). A
    /// [`CommandSender`](crate::actor::CommandSender) — sends through it now
    /// wake the actor (ADR-0050 §D3a).
    pub command_sender: crate::actor::CommandSender,
    /// D7 wall-clock seam.
    pub clock: &'a dyn KernelClock,
    /// Active-account local signing material + active-pubkey accessor.
    pub signers: &'a dyn LocalSignerAccess,
    /// NIP-17 kind:10050 DM-inbox relay reads.
    pub dms: &'a dyn DmInboxLookup,
    /// D6 toast + failure-record surface.
    pub errors: &'a dyn ErrorSurface,
    /// `Requested` action-stage write surface.
    pub stages: &'a dyn ActionStageTracker,
    /// V-07 recipient-relay router wrapper.
    pub recipients: &'a dyn RecipientRelayLookup,
    /// ADR-0052 §D4 — the per-app host-op handler slot accessor. Read by the
    /// `HostOpCommand` in [`crate::substrate::host_op`]; the noop singleton is
    /// installed for every other command (they never call it).
    pub host_op_handler: &'a dyn HostOpHandlerAccess,
    /// ADR-0052 §D5 — the narrow wallet kernel-mutation surface (replaces the
    /// deleted `kernel_mut()`). Only the NIP-47 wallet commands drive it.
    pub wallet_kernel: &'a dyn WalletKernelAccess,
    /// ADR-0052 §D5 — the zap-only cached-profile read (replaces the deleted
    /// generic `lnurl_for_pubkey`). Only the NIP-57 zap command reads it.
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
    /// ADR-0052 §D4 — per-app host-op handler slot accessor.
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

    /// ADR-0043 Decision 2 — the generic, backend-transparent sign-account
    /// port helper. Build an [`ActorCommand::SignEventForAccount`] for
    /// `unsigned` (signed with the active account when `signer_pubkey` is
    /// `None`, else the named roster key) carrying `continuation`, and send it
    /// back into the actor loop.
    ///
    /// Local-vs-bunker is invisible to the caller: the actor's dispatch arm
    /// resolves a local key inline and parks a NIP-46 bunker op; the
    /// continuation is invoked on the actor thread either way, with the
    /// resolved [`SignedEvent`] or an error string. The continuation must only
    /// enqueue further work (e.g. spawn an HTTP worker), never block (D8). It
    /// never receives raw key bytes (D13).
    ///
    /// A worker thread that already holds a [`command_sender_clone`] should use
    /// [`build_sign_event_for_account`] instead — this method exists for command
    /// bodies that still hold the `ctx` on the actor thread.
    pub fn sign_event_for_account(
        &self,
        unsigned: crate::substrate::UnsignedEvent,
        signer_pubkey: Option<String>,
        continuation: impl FnOnce(Result<crate::substrate::SignedEvent, String>) + Send + 'static,
    ) {
        self.send(build_sign_event_for_account(
            unsigned,
            signer_pubkey,
            continuation,
        ));
    }

    /// ADR-0052 §D5 — borrow the narrow [`WalletKernelAccess`] capability (the
    /// NIP-47 wallet runtime's bounded kernel-mutation surface). Replaces the
    /// deleted `kernel_mut()`: a wallet command drives its nine kernel methods
    /// and nothing else of the kernel.
    #[must_use]
    pub fn wallet_kernel(&self) -> &dyn WalletKernelAccess {
        self.wallet_kernel
    }

    /// V-38: Push outbound relay frames produced synchronously by the command
    /// body. The actor's dispatch arm drains them into the existing
    /// `send_all_outbound` plumbing. No-op when no outbound sink is attached
    /// (unit tests).
    pub fn push_outbound<I: IntoIterator<Item = OutboundMessage>>(&mut self, frames: I) {
        if let Some(out) = self.outbound.as_mut() {
            out.extend(frames);
        }
    }

    /// Borrow the [`KernelClock`] capability.
    #[must_use]
    pub fn clock(&self) -> &dyn KernelClock {
        self.clock
    }

    /// Borrow the [`LocalSignerAccess`] capability.
    #[must_use]
    pub fn signers(&self) -> &dyn LocalSignerAccess {
        self.signers
    }

    /// Borrow the [`DmInboxLookup`] capability.
    #[must_use]
    pub fn dms(&self) -> &dyn DmInboxLookup {
        self.dms
    }

    /// Borrow the [`ErrorSurface`] capability.
    #[must_use]
    pub fn errors(&self) -> &dyn ErrorSurface {
        self.errors
    }

    /// Borrow the [`ActionStageTracker`] capability.
    #[must_use]
    pub fn stages(&self) -> &dyn ActionStageTracker {
        self.stages
    }

    /// Borrow the [`RecipientRelayLookup`] capability.
    #[must_use]
    pub fn recipients(&self) -> &dyn RecipientRelayLookup {
        self.recipients
    }

    /// ADR-0052 §D4 — clone the currently-installed host-op handler out of the
    /// per-app slot (`None` when none is installed). D15-wrapped: a panicking
    /// slot accessor falls back to `None` (the genuinely-absent-handler
    /// branch) rather than unwinding the calling `ProtocolCommand::run` frame.
    #[must_use]
    pub fn host_op_handler(
        &self,
    ) -> Option<std::sync::Arc<dyn crate::substrate::HostOpHandler>> {
        let h = self.host_op_handler;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| h.current_handler()))
            .unwrap_or(None)
    }

    // ── D15 catch_unwind shortcuts ──
    //
    // The accessors below wrap a capability call in `catch_unwind` so a
    // panicking host-side adapter cannot unwind the calling
    // `ProtocolCommand::run` frame. NIP commands MAY call the capability
    // method directly via `ctx.clock().now_secs()` etc., but these
    // shortcuts make the panic-safety explicit at the call site (every
    // previous accessor had a `catch_unwind` wrapper; the shortcuts
    // preserve that contract).

    /// Wall-clock seconds since the Unix epoch (D15-wrapped
    /// [`KernelClock::now_secs`]). Returns `0` on a panicking adapter.
    ///
    /// ADR-0052 §D5: always goes through the [`KernelClock`] capability — the
    /// prior kernel-direct fast-path (which dodged the now-deleted `with_kernel`
    /// exclusive borrow) is gone.
    pub fn now_secs(&self) -> u64 {
        let c = self.clock;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.now_secs())).unwrap_or(0)
    }

    /// D15-wrapped [`LocalSignerAccess::active_local_keys`]. Returns
    /// `None` on a panicking adapter (matches the genuinely-absent
    /// account branch).
    #[must_use]
    pub fn active_local_keys(&self) -> Option<nostr::Keys> {
        let s = self.signers;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.active_local_keys()))
            .unwrap_or(None)
    }

    /// D15-wrapped [`LocalSignerAccess::active_account_pubkey`] — the §D5
    /// account-pin source. Returns `None` on a panicking adapter (matches the
    /// genuinely-absent account branch).
    #[must_use]
    pub fn active_account_pubkey(&self) -> Option<String> {
        let s = self.signers;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.active_account_pubkey()))
            .unwrap_or(None)
    }

    /// ADR-0050 §D1 cipher-port helper — the NIP-44 encrypt twin of
    /// [`sign_event_for_account`](Self::sign_event_for_account). Sends an
    /// [`ActorCommand::Nip44EncryptForAccount`] for `plaintext` → `peer_pubkey`
    /// (named `Some(hex)` or active `None` account). Local-vs-bunker is invisible
    /// (D13 — only ciphertext crosses); the continuation runs on the actor thread
    /// and only enqueues work (D8). Worker threads holding a `command_sender_clone`
    /// use [`build_nip44_encrypt_for_account`] directly.
    pub fn nip44_encrypt_for_account(
        &self,
        peer_pubkey: String,
        plaintext: String,
        signer_pubkey: Option<String>,
        continuation: impl FnOnce(Result<String, String>) + Send + 'static,
    ) {
        self.send(build_nip44_encrypt_for_account(
            peer_pubkey,
            plaintext,
            signer_pubkey,
            continuation,
        ));
    }

    /// D15-wrapped [`DmInboxLookup::dm_inbox_relays`]. Returns `None`
    /// on a panicking adapter (the gift-wrap publish path fails closed
    /// on `None` per NIP-17 § 2).
    #[must_use]
    pub fn dm_inbox_relays(&self, recipient: &str) -> Option<Vec<String>> {
        let d = self.dms;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            d.dm_inbox_relays(recipient)
        }))
        .unwrap_or(None)
    }

    /// D15-wrapped [`ErrorSurface::set_last_error_toast`].
    pub fn set_last_error_toast(&self, message: Option<String>) {
        let e = self.errors;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            e.set_last_error_toast(message);
        }));
    }

    /// D15-wrapped [`ErrorSurface::record_action_failure`].
    pub fn record_action_failure(&self, correlation_id: String, reason: String) {
        let e = self.errors;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            e.record_action_failure(correlation_id, reason);
        }));
    }

    /// D15-wrapped [`ActionStageTracker::record_requested`].
    pub fn record_action_stage_requested(&self, correlation_id: &str) {
        let s = self.stages;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            s.record_requested(correlation_id);
        }));
    }

    /// D15-wrapped [`RecipientRelayLookup::recipient_publish_relays`].
    /// Returns an empty `Vec` on a panicking adapter — matches the
    /// "router returned `Unroutable`" branch (caller decides how to
    /// fall back further).
    #[must_use]
    pub fn recipient_publish_relays(&self, recipient: &str, kind: u32) -> Vec<String> {
        let r = self.recipients;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            r.recipient_publish_relays(recipient, kind)
        }))
        .unwrap_or_default()
    }

    /// ADR-0052 §D5 — borrow the [`ZapProfileLookup`] capability (the zap-only
    /// cached-profile read). Replaces the deleted generic `lnurl_for_pubkey`
    /// accessor; the NIP-57 zap command reads its destination via
    /// `ctx.zap_profiles().lnurl_for_pubkey(pk)`, and no other command can.
    #[must_use]
    pub fn zap_profiles(&self) -> &dyn ZapProfileLookup {
        self.zap_profiles
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

#[path = "protocol/builders.rs"]
mod builders;
pub use builders::{
    build_nip44_decrypt_for_account, build_nip44_encrypt_for_account, build_sign_event_for_account,
};

#[cfg(test)]
#[path = "protocol/tests.rs"]
mod tests;
