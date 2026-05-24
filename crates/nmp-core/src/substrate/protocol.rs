//! `ProtocolCommand` — the write-path substrate seam.
//!
//! Defined by `docs/architecture/crate-boundaries.md` §4.1. Step 1.b of the
//! 12-step migration: pure addition + one new [`crate::ActorCommand`] variant
//! (`Protocol(Box<dyn ProtocolCommand>)`). Step 4 (V-41) widens the context
//! with the kernel + identity accessors the NIP-57 LNURL fetcher needs.
//!
//! The seam exists so NIP crates (NIP-17 DM send, NIP-47 NWC pay, NIP-57
//! LNURL fetch, …) stop adding bespoke variants to [`crate::ActorCommand`].
//! They instead dispatch `ActorCommand::Protocol(Box::new(MyCommand{...}))`
//! and the kernel calls `cmd.run(&mut ctx)`. Step 4 (V-41) is the first
//! migration onto the seam (LNURL fetcher); steps 5–7 follow with
//! `SendGiftWrappedDm` (V-39) and the `Wallet*` variants (V-38).
//!
//! ## Why a wrapper context type (`ProtocolCommandContext`) and not `ActorContext`
//!
//! The spec's §4.1 sketches `&mut ActorContext` as the context arg.
//! [`crate::actor::dispatch::ActorContext`] is intentionally `pub(super)` —
//! exposing it would publish 18 fields' worth of kernel internals to every
//! NIP crate. Instead the dispatch arm constructs a public
//! [`ProtocolCommandContext`] that exposes only what the trait needs through
//! a fixed set of closure-typed accessors that close over the kernel +
//! identity references on the actor thread. NIP crates name no internal
//! types — every operation a `ProtocolCommand::run` body can perform is a
//! method on `ProtocolCommandContext`.
//!
//! ## D15 catch_unwind discipline
//!
//! Every closure passed into the context is host-supplied from the
//! kernel boundary. The accessor methods that invoke them are wrapped in
//! [`std::panic::catch_unwind`] so a panicking accessor cannot unwind the
//! calling `ProtocolCommand::run` frame (which would skip the dispatch
//! arm's clean-up + emit). Read accessors fall back to safe defaults on
//! panic (empty Vec, None, 0); the [`send`](ProtocolCommandContext::send)
//! drop-on-panic is benign (the worker reads no return value).

use std::fmt;
use std::sync::mpsc::Sender;

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
/// Step 1.b shipped only the [`send`](Self::send) closure. Step 4 (V-41)
/// widens the surface with the read accessors the LNURL fetcher needs:
/// the wall clock ([`now_secs`](Self::now_secs)), the author's NIP-65 write
/// set ([`author_write_relays`](Self::author_write_relays) +
/// [`bootstrap_discovery_relays`](Self::bootstrap_discovery_relays) cold-start
/// fallback), and the active account's local signing key
/// ([`active_local_keys`](Self::active_local_keys), `None` for NIP-46 bunker
/// accounts). The dispatch arm also tracks the `Requested` action stage
/// via [`record_action_stage_requested`](Self::record_action_stage_requested)
/// when a `correlation_id` is in flight.
///
/// NIP crates never name `Kernel` / `IdentityRuntime` (both crate-private).
/// They only see this `Context` — the operations every protocol command may
/// perform on the actor thread.
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
    now_secs: &'a dyn Fn() -> u64,
    author_write_relays: &'a dyn Fn(&str) -> Vec<String>,
    bootstrap_discovery_relays: &'a dyn Fn() -> Vec<String>,
    active_local_keys: &'a dyn Fn() -> Option<nostr::Keys>,
    record_action_stage_requested: &'a dyn Fn(&str),
}

impl<'a> ProtocolCommandContext<'a> {
    /// Construct the production context — used by the kernel dispatch arm.
    /// Every closure closes over the actor thread's mutable references to
    /// the kernel + identity runtime; the resulting context's lifetime is
    /// the dispatch arm's stack frame.
    #[allow(clippy::too_many_arguments)] // forwarding constructor — adding a builder hides the closure-bundle shape
    pub fn new(
        send: &'a dyn Fn(ActorCommand),
        command_sender: Sender<ActorCommand>,
        now_secs: &'a dyn Fn() -> u64,
        author_write_relays: &'a dyn Fn(&str) -> Vec<String>,
        bootstrap_discovery_relays: &'a dyn Fn() -> Vec<String>,
        active_local_keys: &'a dyn Fn() -> Option<nostr::Keys>,
        record_action_stage_requested: &'a dyn Fn(&str),
    ) -> Self {
        Self {
            send,
            command_sender,
            now_secs,
            author_write_relays,
            bootstrap_discovery_relays,
            active_local_keys,
            record_action_stage_requested,
        }
    }

    /// Test-only constructor that wires only the [`send`](Self::send)
    /// closure. The kernel-access methods (`now_secs`, `author_write_relays`,
    /// `bootstrap_discovery_relays`, `active_local_keys`,
    /// `record_action_stage_requested`) return harmless defaults (0, empty
    /// vec, None, no-op). The `command_sender_clone` returns a sender
    /// whose receiver is immediately dropped — sends become benign no-ops
    /// (matches the D6 "disconnected actor" pattern). Used by trait-level
    /// unit tests in this module and by sibling NIP crate tests that
    /// don't exercise the kernel surface.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_send_only(send: &'a dyn Fn(ActorCommand)) -> Self {
        // Stable noop closures — refs to monomorphisations of the trivial
        // function literals below. They borrow nothing, so the `'a`
        // bound is satisfied for the entire program lifetime.
        const NOW: &dyn Fn() -> u64 = &(|| 0u64);
        const RELAYS_BY_AUTHOR: &dyn Fn(&str) -> Vec<String> = &(|_: &str| Vec::new());
        const BOOTSTRAP: &dyn Fn() -> Vec<String> = &(|| Vec::new());
        const LOCAL_KEYS: &dyn Fn() -> Option<nostr::Keys> = &(|| None);
        const STAGE_REQ: &dyn Fn(&str) = &(|_: &str| ());
        let (command_sender, _rx) = std::sync::mpsc::channel::<ActorCommand>();
        Self {
            send,
            command_sender,
            now_secs: NOW,
            author_write_relays: RELAYS_BY_AUTHOR,
            bootstrap_discovery_relays: BOOTSTRAP,
            active_local_keys: LOCAL_KEYS,
            record_action_stage_requested: STAGE_REQ,
        }
    }

    /// Test-only constructor variant that injects a pre-built command
    /// sender alongside the noop kernel accessors. Used by NIP crate unit
    /// tests that need to drive a `ProtocolCommand::run` body whose
    /// worker thread sends follow-ups and observe the receiver side.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_send_and_sender(
        send: &'a dyn Fn(ActorCommand),
        command_sender: Sender<ActorCommand>,
    ) -> Self {
        const NOW: &dyn Fn() -> u64 = &(|| 0u64);
        const RELAYS_BY_AUTHOR: &dyn Fn(&str) -> Vec<String> = &(|_: &str| Vec::new());
        const BOOTSTRAP: &dyn Fn() -> Vec<String> = &(|| Vec::new());
        const LOCAL_KEYS: &dyn Fn() -> Option<nostr::Keys> = &(|| None);
        const STAGE_REQ: &dyn Fn(&str) = &(|_: &str| ());
        Self {
            send,
            command_sender,
            now_secs: NOW,
            author_write_relays: RELAYS_BY_AUTHOR,
            bootstrap_discovery_relays: BOOTSTRAP,
            active_local_keys: LOCAL_KEYS,
            record_action_stage_requested: STAGE_REQ,
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
    /// (matches the D6 "disconnected actor" pattern). NIP crate tests
    /// that need to observe the worker's follow-ups must use
    /// [`Self::with_send_and_sender`] (or, when the kernel surface
    /// matters, the production [`Self::new`]).
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

    /// Wall-clock seconds since the Unix epoch, read from the kernel's
    /// authoritative clock (D7 — the kernel owns the wall clock; protocol
    /// commands MUST NOT call `SystemTime::now` directly). Returns `0` on a
    /// panicking accessor (defensive — matches the `created_at = 0` sentinel
    /// the publish path already understands as "not stamped").
    pub fn now_secs(&self) -> u64 {
        let f = self.now_secs;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f())).unwrap_or(0)
    }

    /// The author's NIP-65 (kind:10002) write-relay list. Falls back to the
    /// bootstrap discovery seed when no kind:10002 is cached yet (cold start).
    /// Returns an empty `Vec` on a panicking accessor (D15 fallback).
    #[must_use]
    pub fn author_write_relays(&self, author: &str) -> Vec<String> {
        let f = self.author_write_relays;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(author))).unwrap_or_default()
    }

    /// The bootstrap discovery relays seed — used when no per-author
    /// kind:10002 hint is available (cold-start fallback in routing /
    /// recipient relay injection paths). Returns an empty `Vec` on a
    /// panicking accessor (D15 fallback).
    #[must_use]
    pub fn bootstrap_discovery_relays(&self) -> Vec<String> {
        let f = self.bootstrap_discovery_relays;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f())).unwrap_or_default()
    }

    /// The active account's local `nostr::Keys`, cloned. `None` for
    /// NIP-46 bunker accounts (ADR-0026 Phase 1 — remote-signer signing of
    /// arbitrary unsigned events is a follow-up). Protocol commands that
    /// need to mint a signature on the actor thread (NIP-57 kind:9734,
    /// NIP-59 seal+gift-wrap) read keys through this seam.
    /// Returns `None` on a panicking accessor (D15 fallback — same as a
    /// genuinely-absent account).
    #[must_use]
    pub fn active_local_keys(&self) -> Option<nostr::Keys> {
        let f = self.active_local_keys;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f())).unwrap_or(None)
    }

    /// Record a `Requested` stage for `correlation_id` against the kernel's
    /// `action_stages` mirror. Idempotent — the kernel's
    /// `record_action_stage` appends a history row; a re-record is a benign
    /// duplicate. The LNURL fetcher dispatch arm calls this for the
    /// `correlation_id` it received (when present) so the host stage observer
    /// sees the `Requested` transition before the worker thread fires the
    /// terminal `Accepted` / `Failed`.
    pub fn record_action_stage_requested(&self, correlation_id: &str) {
        let f = self.record_action_stage_requested;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(correlation_id)));
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

    // V-41 — the read accessors return safe defaults under
    // `with_send_only` so NIP crate tests that don't exercise kernel state
    // can still drive a `ProtocolCommand::run` body.

    #[test]
    fn with_send_only_defaults_are_safe() {
        let send = |_: ActorCommand| {};
        let ctx = ProtocolCommandContext::with_send_only(&send);
        assert_eq!(ctx.now_secs(), 0);
        assert!(ctx.author_write_relays("anything").is_empty());
        assert!(ctx.bootstrap_discovery_relays().is_empty());
        assert!(ctx.active_local_keys().is_none());
        // Recording a stage on a no-op should not panic.
        ctx.record_action_stage_requested("cid-noop");
    }

    // The full constructor threads its closures through unchanged — the
    // V-41 dispatch arm relies on this directly.
    #[test]
    fn full_constructor_threads_closures() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::mpsc;

        let send = |_: ActorCommand| {};
        let clock = AtomicU64::new(123_456);
        let now_secs = || clock.load(Ordering::SeqCst);
        let stub_relays = || vec!["wss://stub.example".to_string()];
        let by_author = |a: &str| vec![format!("wss://{a}.example")];
        let local_keys = || None::<nostr::Keys>;
        let stage_seen: Mutex<Vec<String>> = Mutex::new(Vec::new());
        let stage_req = |cid: &str| stage_seen.lock().unwrap().push(cid.to_string());
        let (tx, rx) = mpsc::channel::<ActorCommand>();

        let ctx = ProtocolCommandContext::new(
            &send,
            tx,
            &now_secs,
            &by_author,
            &stub_relays,
            &local_keys,
            &stage_req,
        );

        assert_eq!(ctx.now_secs(), 123_456);
        assert_eq!(
            ctx.author_write_relays("alice"),
            vec!["wss://alice.example".to_string()]
        );
        assert_eq!(
            ctx.bootstrap_discovery_relays(),
            vec!["wss://stub.example".to_string()]
        );
        assert!(ctx.active_local_keys().is_none());
        ctx.record_action_stage_requested("cid-abc");
        assert_eq!(*stage_seen.lock().unwrap(), vec!["cid-abc".to_string()]);
        // The worker-side sender clone reaches the matching receiver.
        let cloned = ctx.command_sender_clone();
        cloned.send(ActorCommand::Shutdown).expect("send");
        match rx.recv().unwrap() {
            ActorCommand::Shutdown => (),
            other => panic!("expected Shutdown, got {other:?}"),
        }
    }

    #[test]
    fn with_send_only_provides_disconnected_sender() {
        // The sender is real (clones cleanly) but the receiver is
        // immediately dropped — sends are benign no-ops, matching the
        // D6 "actor already shut down" pattern. Worker code is free to
        // call `send` and observe the error, which it must swallow.
        let send = |_: ActorCommand| {};
        let ctx = ProtocolCommandContext::with_send_only(&send);
        let cloned = ctx.command_sender_clone();
        // No receiver → send returns Err, but the contract allows the
        // caller to swallow it (D6). Verify it is in fact a send-error
        // shape, not a panic.
        assert!(cloned.send(ActorCommand::Shutdown).is_err());
    }
}
