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
//! Prior to Debt C the dispatch arm threaded 12 individual closures
//! into [`ProtocolCommandContext::new`] (with `#[allow(clippy::too_many_arguments)]`).
//! That bundle has been replaced by 5 typed capability traits the dispatch
//! arm wires in once:
//!
//! - [`KernelClock`] — wall-clock seam (D7).
//! - [`LocalSignerAccess`] — local `nostr::Keys` snapshot + the V-08
//!   `SignerForSeal` resolver that uniformly handles BOTH local-nsec
//!   accounts AND NIP-46 bunker accounts on the gift-wrap path.
//! - [`DmInboxLookup`] — kind:10050 DM-inbox relay reads. Substrate-
//!   generic; the concrete kind:10050 cache lives in `nmp-nip17`.
//! - [`ErrorSurface`] — D6 observable error writes: the
//!   `last_error_toast` projection plus the `Failed` action-stage
//!   recorder. The DM send and LNURL send paths both fire on every
//!   early-exit branch.
//! - [`ActionStageTracker`] — the `Requested` action stage write the
//!   substrate arm performs when a `correlation_id` is in flight.
//!
//! `ProtocolCommandContext::new` now takes 7 args (`send`,
//! `command_sender`, plus the five `&dyn Capability` references). NIP
//! commands call `ctx.clock().now_secs()`, `ctx.signers().signer_for_seal()`,
//! `ctx.dms().dm_inbox_relays(pk)`, etc. — the trait names tell every reader
//! which surface a given call belongs to.
//!
//! ## Routing accessors removed (Debt A + Debt C overlap)
//!
//! `author_write_relays` and `bootstrap_discovery_relays` were removed.
//! Routing is the kernel's `OutboxRouter` — NIP commands that need a
//! recipient relay set MUST route via `OutboxRouter::route_publish` (or
//! populate `RoutingContext::explicit_targets`). The NIP-57 LNURL fetcher
//! that previously consulted those accessors carries a
//! `// TODO Debt C follow-up` until its routing path is migrated.
//!
//! ## Why a wrapper context type (`ProtocolCommandContext`) and not `ActorContext`
//!
//! The spec's §4.1 sketches `&mut ActorContext` as the context arg.
//! [`crate::actor::dispatch::ActorContext`] is intentionally `pub(super)` —
//! exposing it would publish 18 fields' worth of kernel internals to every
//! NIP crate. Instead the dispatch arm constructs a public
//! [`ProtocolCommandContext`] that exposes only what the trait needs through
//! a fixed set of capability accessors. NIP crates name no internal
//! types — every operation a `ProtocolCommand::run` body can perform is a
//! method on `ProtocolCommandContext`.
//!
//! ## D15 catch_unwind discipline
//!
//! Every accessor that fires a capability method is wrapped in
//! [`std::panic::catch_unwind`] so a panicking host-side adapter cannot
//! unwind the calling `ProtocolCommand::run` frame (which would skip the
//! dispatch arm's clean-up + emit). Read accessors fall back to safe
//! defaults on panic (empty Vec, None, 0); the
//! [`send`](ProtocolCommandContext::send) drop-on-panic is benign (the
//! worker reads no return value).

use std::fmt;
use std::sync::mpsc::Sender;
use std::sync::Arc;

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

// ──────────────────────────────────────────────────────────────────────────
// ProtocolCommandContext
// ──────────────────────────────────────────────────────────────────────────

/// Per-command runtime affordances handed to [`ProtocolCommand::run`].
///
/// Post-Debt C the context exposes 5 typed capability traits
/// ([`KernelClock`], [`LocalSignerAccess`], [`DmInboxLookup`],
/// [`ErrorSurface`], [`ActionStageTracker`]) plus the two channel-shaped
/// primitives ([`send`](Self::send) and
/// [`command_sender_clone`](Self::command_sender_clone)). The previous
/// 12-positional-closure constructor is gone — `new()` takes 7 args.
///
/// NIP crates never name `Kernel` / `IdentityRuntime` (both crate-private).
/// They only see this context and the capability traits.
pub struct ProtocolCommandContext<'a> {
    send: &'a dyn Fn(ActorCommand),
    /// Owned actor-command sender clone the command's `run` body can hand
    /// to a spawned worker thread. The actor thread's own
    /// `Sender<ActorCommand>` is `'static`-clone-friendly, so this is a
    /// plain owned `Sender` rather than another `&dyn Fn`. Always populated
    /// by every constructor; the test-only `with_send_only` constructor
    /// installs a fresh `mpsc::channel`'s sender with a discarded receiver
    /// (sends become benign drops, matching D6 semantics for a
    /// disconnected actor).
    command_sender: Sender<ActorCommand>,
    clock: &'a dyn KernelClock,
    signers: &'a dyn LocalSignerAccess,
    dms: &'a dyn DmInboxLookup,
    errors: &'a dyn ErrorSurface,
    stages: &'a dyn ActionStageTracker,
}

impl<'a> ProtocolCommandContext<'a> {
    /// Construct the production context — used by the kernel dispatch arm.
    /// The 5 capability references close over the actor thread's mutable
    /// references to the kernel + identity runtime; the resulting context's
    /// lifetime is the dispatch arm's stack frame.
    pub fn new(
        send: &'a dyn Fn(ActorCommand),
        command_sender: Sender<ActorCommand>,
        clock: &'a dyn KernelClock,
        signers: &'a dyn LocalSignerAccess,
        dms: &'a dyn DmInboxLookup,
        errors: &'a dyn ErrorSurface,
        stages: &'a dyn ActionStageTracker,
    ) -> Self {
        Self {
            send,
            command_sender,
            clock,
            signers,
            dms,
            errors,
            stages,
        }
    }

    /// Test-only constructor that wires only the [`send`](Self::send)
    /// closure. All capability accessors return harmless defaults (0,
    /// None, no-op) via the [`NoopKernelClock`] / [`NoopLocalSignerAccess`]
    /// / [`crate::substrate::EmptyDmInboxRelayLookup`] / [`NoopErrorSurface`]
    /// / [`NoopActionStageTracker`] noop singletons. The `command_sender_clone`
    /// returns a sender whose receiver is immediately dropped — sends become
    /// benign no-ops (matches the D6 "disconnected actor" pattern).
    ///
    /// Used by trait-level unit tests in this module and by sibling NIP
    /// crate tests that don't exercise the kernel surface. Tests that DO
    /// need a specific capability construct a small local adapter struct
    /// implementing the relevant trait and pass it through [`Self::new`].
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_send_only(send: &'a dyn Fn(ActorCommand)) -> Self {
        static CLOCK: NoopKernelClock = NoopKernelClock;
        static SIGNERS: NoopLocalSignerAccess = NoopLocalSignerAccess;
        static DMS: crate::substrate::EmptyDmInboxRelayLookup =
            crate::substrate::EmptyDmInboxRelayLookup;
        static ERRORS: NoopErrorSurface = NoopErrorSurface;
        static STAGES: NoopActionStageTracker = NoopActionStageTracker;
        let (command_sender, _rx) = std::sync::mpsc::channel::<ActorCommand>();
        Self {
            send,
            command_sender,
            clock: &CLOCK,
            signers: &SIGNERS,
            dms: &DMS,
            errors: &ERRORS,
            stages: &STAGES,
        }
    }

    /// Return an owned [`Sender<ActorCommand>`] clone the command's `run`
    /// body can hand to a spawned worker thread. The worker uses it to
    /// post follow-up `ActorCommand`s back into the actor loop after the
    /// dispatch arm (and therefore the `ProtocolCommandContext`) has
    /// returned (the LNURL fetcher pattern — see
    /// `nmp_nip57::lnurl::FetchLnurlInvoiceCommand`).
    ///
    /// The test-only `with_send_only` constructor installs a sender whose
    /// receiver is immediately dropped — sends become benign no-ops
    /// (matches the D6 "disconnected actor" pattern).
    #[must_use]
    pub fn command_sender_clone(&self) -> Sender<ActorCommand> {
        self.command_sender.clone()
    }

    /// Re-enter the actor loop with `cmd`. The actor processes it in a
    /// subsequent dispatch cycle (same thread, same channel).
    ///
    /// D15: the closure is host-supplied (constructed by the dispatch
    /// arm but conceptually owned by the kernel boundary), so the
    /// invocation is wrapped in [`std::panic::catch_unwind`] to keep a
    /// panicking follow-up from unwinding the calling `ProtocolCommand`'s
    /// `run()` frame.
    pub fn send(&self, cmd: ActorCommand) {
        let send = self.send;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| send(cmd)));
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

    // ── D15 catch_unwind shortcuts ──
    //
    // The five accessors below wrap a capability call in `catch_unwind` so
    // a panicking host-side adapter cannot unwind the calling
    // `ProtocolCommand::run` frame. NIP commands MAY call the capability
    // method directly via `ctx.clock().now_secs()` etc., but these
    // shortcuts make the panic-safety explicit and concise at the call
    // site (every previous accessor had a `catch_unwind` wrapper; the
    // shortcuts preserve that contract).

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
        assert!(ctx.signer_for_seal().is_none());
        assert!(ctx.dm_inbox_relays("anything").is_none());
        ctx.set_last_error_toast(Some("toast".to_string()));
        ctx.record_action_failure("cid".to_string(), "err".to_string());
        ctx.record_action_stage_requested("cid-noop");
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
        signer: Option<Arc<dyn nmp_nip59::SignerForSeal>>,
    }
    impl LocalSignerAccess for LocalSigners {
        fn active_local_keys(&self) -> Option<nostr::Keys> {
            self.keys.clone()
        }
        fn signer_for_seal(&self) -> Option<Arc<dyn nmp_nip59::SignerForSeal>> {
            self.signer.clone()
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

    #[test]
    fn full_constructor_threads_capabilities() {
        use std::sync::mpsc;
        let send = |_: ActorCommand| {};
        let clock = FixedClock(123_456);
        let signers = LocalSigners { keys: None, signer: None };
        let dms = crate::substrate::EmptyDmInboxRelayLookup;
        let errors = RecordingErrors {
            toasts: Mutex::new(Vec::new()),
            failures: Mutex::new(Vec::new()),
        };
        let stages = RecordingStages { seen: Mutex::new(Vec::new()) };
        let (tx, rx) = mpsc::channel::<ActorCommand>();

        let ctx = ProtocolCommandContext::new(
            &send, tx, &clock, &signers, &dms, &errors, &stages,
        );

        assert_eq!(ctx.now_secs(), 123_456);
        assert!(ctx.active_local_keys().is_none());
        assert!(ctx.signer_for_seal().is_none());
        assert!(ctx.dm_inbox_relays("anyone").is_none());
        ctx.set_last_error_toast(Some("hello".to_string()));
        ctx.record_action_failure("cid-z".to_string(), "boom".to_string());
        ctx.record_action_stage_requested("cid-abc");
        assert_eq!(*errors.toasts.lock().unwrap(), vec![Some("hello".to_string())]);
        assert_eq!(
            *errors.failures.lock().unwrap(),
            vec![("cid-z".to_string(), "boom".to_string())]
        );
        assert_eq!(*stages.seen.lock().unwrap(), vec!["cid-abc".to_string()]);

        // Worker-side sender clone reaches the matching receiver.
        let cloned = ctx.command_sender_clone();
        cloned.send(ActorCommand::Shutdown).expect("send");
        match rx.recv().unwrap() {
            ActorCommand::Shutdown => (),
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
}
