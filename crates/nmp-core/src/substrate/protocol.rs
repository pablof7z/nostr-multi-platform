//! `ProtocolCommand` — the write-path substrate seam.
//!
//! Defined by `docs/architecture/crate-boundaries.md` §4.1. Step 1.b of the
//! 12-step migration: pure addition + one new [`crate::ActorCommand`] variant
//! (`Protocol(Box<dyn ProtocolCommand>)`). Step 4 (V-41) added the kernel +
//! identity accessors the NIP-57 LNURL fetcher needs; V-39+V-40 (NIP-17 DM
//! stack) added the local-keys snapshot, DM-inbox relay lookup, and D6 error
//! surface; V-08 added the `SignerForSeal` resolver for bunker accounts.
//!
//! ## Debt C — capability traits replace a 12-arg closure bundle
//!
//! Pre-Debt C the dispatch arm threaded 12 individual closures into
//! [`ProtocolCommandContext::new`] (with `#[allow(clippy::too_many_arguments)]`).
//! The follow-up (V-41 + V-39+V-40 + V-08 bunker DM) reduced the
//! constructor to 6 typed capability traits plus 2 channel-shaped sinks
//! (`send`, `command_sender`). A subsequent collapse pass folded those 8
//! positional args into a single named-field [`ProtocolCommandContextParts`]
//! struct so the constructor takes one arg and call sites read top-to-bottom
//! as a complete construction recipe. D11 still holds: there is exactly
//! one public production constructor, [`ProtocolCommandContext::new`]; the
//! test-only [`ProtocolCommandContext::with_send_only`] is gated behind
//! `cfg(any(test, feature = "test-support"))`.
//!
//! Capability traits bundled by the parts struct:
//!
//! - [`KernelClock`] — D7 wall-clock seam.
//! - [`LocalSignerAccess`] — local `nostr::Keys` snapshot + V-08
//!   `SignerForSeal` resolver (covers BOTH local-nsec AND NIP-46 bunker).
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
//! NIP commands call `ctx.clock().now_secs()`, `ctx.signers().signer_for_seal()`,
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
use std::sync::mpsc::Sender;
use std::sync::Arc;

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

// ──────────────────────────────────────────────────────────────────────────
// Capability traits (Debt C — replaces the 12-positional-closure bundle)
// ──────────────────────────────────────────────────────────────────────────

/// D7 — kernel-owned wall clock. NIP commands MUST read time through this
/// seam rather than calling `SystemTime::now` directly.
pub trait KernelClock: Send + Sync {
    /// Seconds since the Unix epoch.
    fn now_secs(&self) -> u64;
}

/// Active-account local signing material. Used by NIP commands that need
/// to mint a signature on the actor thread (NIP-57 kind:9734 signing,
/// NIP-17 gift-wrap sealing).
pub trait LocalSignerAccess: Send + Sync {
    /// Active account's local `nostr::Keys`, cloned. `None` for NIP-46
    /// bunker accounts (which expose signing through [`Self::signer_for_seal`]
    /// instead) and when no account is active.
    fn active_local_keys(&self) -> Option<nostr::Keys>;

    /// V-08 — resolve a [`SignerForSeal`][nmp_nip59::SignerForSeal] that
    /// uniformly handles BOTH local-nsec accounts (blanket impl on
    /// `nostr::Keys`, every chain step `Ready`) AND NIP-46 bunker accounts
    /// (`RemoteSignerForSeal` adapter — `nip44_encrypt` + `sign_seal` run
    /// `Pending` on a per-invocation driver thread). `None` when no account
    /// is active or a remote signer reported a malformed pubkey.
    fn signer_for_seal(&self) -> Option<Arc<dyn nmp_nip59::SignerForSeal>>;
}

/// NIP-17 kind:10050 DM-inbox relay reads — substrate-generic. Re-uses
/// the existing [`crate::substrate::DmInboxRelayLookup`] trait (the same
/// seam the planner's kernel-side `MailboxCache` adapter consults). The
/// concrete cache lives in `nmp-nip17::DmRelayCache`; this re-export
/// keeps the capability-trait surface consistent (one name for the
/// DM-inbox lookup contract across the substrate).
pub use crate::substrate::DmInboxRelayLookup as DmInboxLookup;

/// D6 observable error surfaces — the `last_error_toast` projection and
/// the `Failed` terminal action-stage recorder. NIP commands fire these
/// on every early-exit branch so the host's spinner clears.
pub trait ErrorSurface: Send + Sync {
    /// Write the `last_error_toast` projection. `None` clears the toast.
    fn set_last_error_toast(&self, message: Option<String>);

    /// Record a `Failed` terminal stage for `correlation_id` with
    /// `reason` as the failure message.
    fn record_action_failure(&self, correlation_id: String, reason: String);
}

/// Action-stage write surface — the `Requested` transition recorded
/// against an in-flight `correlation_id`. Idempotent.
pub trait ActionStageTracker: Send + Sync {
    /// Record a `Requested` stage for `correlation_id`.
    fn record_requested(&self, correlation_id: &str);
}

/// Recipient-relay lookup surface — the substrate-level wrapper around
/// `OutboxRouter::route_publish` that NIP commands need to materialise a
/// recipient's "where would your followers / your own outbox publish a
/// kind:K event under your authorship?" relay set. Concretely: the NIP-57
/// LNURL fetcher's kind:9734 `relays` tag must carry the recipient's
/// NIP-65 write list so the LN provider knows where to publish the
/// kind:9735 zap receipt (NIP-57 § "Appendix F").
///
/// This is **not** a bare cache accessor. The kernel-side adapter drives
/// the injected `outbox_router` slot with a synthetic publish-direction
/// `UnsignedEvent { pubkey: recipient, kind, .. }`; the router's lane 1
/// resolves to the cached NIP-65 write set, lane 7 falls back to the
/// AppRelay cold-start seed. NIP crates therefore never read the
/// substrate `MailboxCache` directly — they go through the router via
/// this capability (Debt-A: router is the live decision authority).
pub trait RecipientRelayLookup: Send + Sync {
    /// Resolve the relay URLs the LN provider (or analogous downstream
    /// publisher) should publish a `kind`-typed event authored by
    /// `recipient` to. Empty `Vec` when the router returns `Unroutable`
    /// (no NIP-65 cache hit AND no AppRelay seed) — the caller decides
    /// whether to fall back further or surface the empty tag.
    ///
    /// `kind` is the synthetic event kind the router uses to drive
    /// lane-6 / lane-7 discriminators; pass the kind the downstream
    /// publication carries (e.g. `9735` for NIP-57 zap-receipt routing).
    fn recipient_publish_relays(&self, recipient: &str, kind: u32) -> Vec<String>;
}

// ──────────────────────────────────────────────────────────────────────────
// Noop default impls — used by `with_send_only` and as fall-throughs for
// NIP crate tests that don't exercise a given capability surface.
// ──────────────────────────────────────────────────────────────────────────

/// Noop [`KernelClock`] — returns `0`. Used as the `with_send_only`
/// default and by NIP crate tests that don't need a real clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopKernelClock;

impl KernelClock for NoopKernelClock {
    fn now_secs(&self) -> u64 {
        0
    }
}

/// Noop [`LocalSignerAccess`] — returns `None` for both accessors.
/// Mirrors the "not signed in" branch.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopLocalSignerAccess;

impl LocalSignerAccess for NoopLocalSignerAccess {
    fn active_local_keys(&self) -> Option<nostr::Keys> {
        None
    }
    fn signer_for_seal(&self) -> Option<Arc<dyn nmp_nip59::SignerForSeal>> {
        None
    }
}

/// Noop [`ErrorSurface`] — discards toasts and failure recordings.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopErrorSurface;

impl ErrorSurface for NoopErrorSurface {
    fn set_last_error_toast(&self, _message: Option<String>) {}
    fn record_action_failure(&self, _correlation_id: String, _reason: String) {}
}

/// Noop [`ActionStageTracker`] — discards stage transitions.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopActionStageTracker;

impl ActionStageTracker for NoopActionStageTracker {
    fn record_requested(&self, _correlation_id: &str) {}
}

/// Noop [`RecipientRelayLookup`] — returns an empty `Vec` for every
/// recipient. Mirrors the "router not wired / no NIP-65 cached" branch;
/// the [`with_send_only`](ProtocolCommandContext::with_send_only) default
/// and NIP crate tests that don't exercise the routing surface install
/// this singleton.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopRecipientRelayLookup;

impl RecipientRelayLookup for NoopRecipientRelayLookup {
    fn recipient_publish_relays(&self, _recipient: &str, _kind: u32) -> Vec<String> {
        Vec::new()
    }
}

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
    /// to a spawned worker thread (the LNURL fetcher pattern).
    pub command_sender: Sender<ActorCommand>,
    /// D7 wall-clock seam.
    pub clock: &'a dyn KernelClock,
    /// Active-account local signing material (incl. `SignerForSeal`).
    pub signers: &'a dyn LocalSignerAccess,
    /// NIP-17 kind:10050 DM-inbox relay reads.
    pub dms: &'a dyn DmInboxLookup,
    /// D6 toast + failure-record surface.
    pub errors: &'a dyn ErrorSurface,
    /// `Requested` action-stage write surface.
    pub stages: &'a dyn ActionStageTracker,
    /// V-07 recipient-relay router wrapper.
    pub recipients: &'a dyn RecipientRelayLookup,
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
    /// Owned `Sender<ActorCommand>` clone for handing to a spawned worker
    /// thread; the test-only `with_send_only` ctor installs a sender whose
    /// receiver is dropped (sends become benign no-ops, matching D6).
    command_sender: Sender<ActorCommand>,
    clock: &'a dyn KernelClock,
    signers: &'a dyn LocalSignerAccess,
    dms: &'a dyn DmInboxLookup,
    errors: &'a dyn ErrorSurface,
    stages: &'a dyn ActionStageTracker,
    recipients: &'a dyn RecipientRelayLookup,
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
    /// Construct from a [`ProtocolCommandContextParts`] bundle (the sole
    /// public production door). Capability references close over the
    /// dispatch arm's stack-bound borrows of kernel + identity runtime;
    /// the resulting context's lifetime is the dispatch arm's stack frame.
    ///
    /// V-38: `kernel` + `outbound` start as `None`; attach them via the
    /// [`with_kernel`](Self::with_kernel) / [`with_outbound`](Self::with_outbound)
    /// builders from the dispatch arm.
    pub fn new(parts: ProtocolCommandContextParts<'a>) -> Self {
        let ProtocolCommandContextParts {
            send, command_sender, clock, signers, dms, errors, stages, recipients,
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
            kernel: None,
            outbound: None,
        }
    }

    /// V-38 builder: attach the actor's kernel handle. The dispatch arm
    /// calls this before invoking [`ProtocolCommand::run`].
    #[must_use]
    pub fn with_kernel(mut self, kernel: &'a mut Kernel) -> Self {
        self.kernel = Some(kernel);
        self
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
        let (command_sender, _rx) = std::sync::mpsc::channel::<ActorCommand>();
        Self::new(ProtocolCommandContextParts {
            send,
            command_sender,
            clock: &CLOCK,
            signers: &SIGNERS,
            dms: &DMS,
            errors: &ERRORS,
            stages: &STAGES,
            recipients: &RECIPIENTS,
        })
    }

    /// Return an owned [`Sender<ActorCommand>`] clone for handing to a
    /// spawned worker thread that posts follow-up `ActorCommand`s back
    /// into the actor loop after the dispatch arm (and therefore this
    /// `ProtocolCommandContext`) has returned — the LNURL fetcher pattern
    /// (`nmp_nip57::lnurl::FetchLnurlInvoiceCommand`). The test-only
    /// `with_send_only` ctor installs a sender whose receiver is dropped
    /// (sends become benign no-ops, matching D6).
    #[must_use]
    pub fn command_sender_clone(&self) -> Sender<ActorCommand> {
        self.command_sender.clone()
    }

    /// Re-enter the actor loop with `cmd`. D15: the host-supplied closure
    /// is wrapped in [`std::panic::catch_unwind`] so a panicking follow-up
    /// cannot unwind the calling `ProtocolCommand::run` frame.
    pub fn send(&self, cmd: ActorCommand) {
        let send = self.send;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| send(cmd)));
    }

    /// V-38: Reborrow the actor's kernel handle. `None` only in unit tests
    /// that constructed the context without one.
    pub fn kernel_mut(&mut self) -> Option<&mut Kernel> {
        self.kernel.as_deref_mut()
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

    /// D15-wrapped [`LocalSignerAccess::signer_for_seal`]. Returns
    /// `None` on a panicking adapter (matches the genuinely-absent
    /// signer branch).
    #[must_use]
    pub fn signer_for_seal(&self) -> Option<Arc<dyn nmp_nip59::SignerForSeal>> {
        let s = self.signers;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.signer_for_seal()))
            .unwrap_or(None)
    }

    /// D15-wrapped [`DmInboxLookup::dm_inbox_relays`]. Returns `None`
    /// on a panicking adapter (the gift-wrap publish path fails closed
    /// on `None` per NIP-17 § 2).
    #[must_use]
    pub fn dm_inbox_relays(&self, recipient: &str) -> Option<Vec<String>> {
        let d = self.dms;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| d.dm_inbox_relays(recipient)))
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
#[path = "protocol/tests.rs"]
mod tests;
