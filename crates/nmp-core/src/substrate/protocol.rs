//! `ProtocolCommand` — the write-path substrate seam.
//!
//! Defined by `docs/architecture/crate-boundaries.md` §4.1. Step 1.b of the
//! 12-step migration: pure addition + one new [`crate::ActorCommand`] variant
//! (`Protocol(Box<dyn ProtocolCommand>)`). Step 4 (V-41) widens the context
//! with the kernel + identity accessors the NIP-57 LNURL fetcher needs.
//! V-39+V-40 (NIP-17 DM stack) widens it further with the local-keys snapshot,
//! the DM-inbox relay lookup, and the D6 error-surface closures the gift-wrap
//! send path uses.
//!
//! The seam exists so NIP crates (NIP-17 DM send, NIP-47 NWC pay, NIP-57
//! LNURL fetch, …) stop adding bespoke variants to [`crate::ActorCommand`].
//! They instead dispatch `ActorCommand::Protocol(Box::new(MyCommand{...}))`
//! and the kernel calls `cmd.run(&mut ctx)`. Step 4 (V-41) was the first
//! migration onto the seam (LNURL fetcher); V-39+V-40 follows with
//! `SendGiftWrappedDm`; V-38 NWC `Wallet*` variants come next.
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
/// V-39+V-40 (NIP-17 DM stack) extends the surface with the affordances the
/// gift-wrap DM send needs:
///
/// * `nip17_local_keys` — cloned `nostr::Keys` for the active local
///   account, or `None` for a remote (NIP-46) signer / not signed in.
///   The DM send path uses these as a `nmp_nip59::SignerForSeal` via the
///   blanket impl on `nostr::Keys` (every `SignerOp::Ready`, so the seal
///   chain runs synchronously on the actor thread). Stored as an owned
///   `Option<nostr::Keys>` (Keys is `Clone` and zeroizes its secret on
///   drop) rather than a closure — the dispatch arm reads it once from
///   the slot.
/// * `dm_inbox_relays` — DM-inbox relay lookup (substrate-generic; the
///   concrete cache lives in `nmp-nip17`). Returns `None` when no list
///   is known for `pubkey`; the gift-wrap publish path fails closed on
///   `None` to keep kind:1059 envelopes off generic Content relays.
/// * `set_last_error_toast` / `record_action_failure` — D6 observable
///   error surfaces. The DM send path writes a toast on every early-exit
///   branch (no active signer, malformed recipient, missing kind:10050,
///   …) and records a `Failed` terminal action stage so the host's
///   spinner clears.
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
    // V-39+V-40 — NIP-17 DM stack surface.
    nip17_local_keys: Option<nostr::Keys>,
    dm_inbox_relays: &'a dyn crate::substrate::DmInboxRelayLookup,
    set_last_error_toast: &'a dyn Fn(Option<String>),
    record_action_failure: &'a dyn Fn(String, String),
    /// V-08 (bunker DM send) — resolve a `SignerForSeal` for the active
    /// account. Returns `Some(Arc<dyn SignerForSeal>)` for **both** local
    /// (nsec) and remote (NIP-46 bunker) accounts — the dispatch arm
    /// routes through `IdentityRuntime::active_signer_for_seal`, which
    /// hands back `Arc<Keys>` for local accounts and a
    /// `RemoteSignerForSeal` adapter for remote handles. `None` when no
    /// account is active OR a remote signer reports a malformed pubkey
    /// (graceful-degrade). Replaces the V-39 local-only
    /// `nip17_local_keys` snapshot on the NIP-17 DM send path.
    signer_for_seal: &'a dyn Fn() -> Option<Arc<dyn nmp_nip59::SignerForSeal>>,
    /// Snapshot override for [`Self::now_secs`]. When `Some`, the accessor
    /// returns it verbatim and skips the `now_secs` closure. The V-39+V-40
    /// `with_now_secs(u64)` builder writes this slot so tests can drive
    /// the DM send path against a deterministic clock without rebuilding
    /// the closure-shaped `now_secs` field. Default `None`; the dispatch
    /// arm reads through the closure as before.
    now_secs_snapshot: Option<u64>,
}

impl<'a> ProtocolCommandContext<'a> {
    /// Construct the production context — used by the kernel dispatch arm.
    /// Every closure closes over the actor thread's mutable references to
    /// the kernel + identity runtime; the resulting context's lifetime is
    /// the dispatch arm's stack frame.
    ///
    /// The trailing four arguments are V-39+V-40 additions for the NIP-17
    /// DM stack: the active account's local keys (read once from
    /// [`crate::ffi::Nip17LocalKeysSlot`] by the dispatch arm), the
    /// kernel-owned DM-inbox relay lookup, and two D6 error-surface
    /// closures.
    #[allow(clippy::too_many_arguments)] // forwarding constructor — adding a builder hides the closure-bundle shape
    pub fn new(
        send: &'a dyn Fn(ActorCommand),
        command_sender: Sender<ActorCommand>,
        now_secs: &'a dyn Fn() -> u64,
        author_write_relays: &'a dyn Fn(&str) -> Vec<String>,
        bootstrap_discovery_relays: &'a dyn Fn() -> Vec<String>,
        active_local_keys: &'a dyn Fn() -> Option<nostr::Keys>,
        record_action_stage_requested: &'a dyn Fn(&str),
        nip17_local_keys: Option<nostr::Keys>,
        dm_inbox_relays: &'a dyn crate::substrate::DmInboxRelayLookup,
        set_last_error_toast: &'a dyn Fn(Option<String>),
        record_action_failure: &'a dyn Fn(String, String),
        signer_for_seal: &'a dyn Fn() -> Option<Arc<dyn nmp_nip59::SignerForSeal>>,
    ) -> Self {
        Self {
            send,
            command_sender,
            now_secs,
            author_write_relays,
            bootstrap_discovery_relays,
            active_local_keys,
            record_action_stage_requested,
            nip17_local_keys,
            dm_inbox_relays,
            set_last_error_toast,
            record_action_failure,
            signer_for_seal,
            now_secs_snapshot: None,
        }
    }

    /// Test-only constructor that wires only the [`send`](Self::send)
    /// closure. All other accessors return harmless defaults (0, empty
    /// vec, None, no-op). The `command_sender_clone` returns a sender
    /// whose receiver is immediately dropped — sends become benign no-ops
    /// (matches the D6 "disconnected actor" pattern). Used by trait-level
    /// unit tests in this module and by sibling NIP crate tests that
    /// don't exercise the kernel surface.
    ///
    /// V-39+V-40 added builder methods (`with_nip17_local_keys`,
    /// `with_dm_inbox_relays`, `with_now_secs`, `with_set_last_error_toast`,
    /// `with_record_action_failure`) — NIP crate tests that need a
    /// specific affordance start with `with_send_only(&send)` and chain
    /// the relevant builders.
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
        const TOAST: &dyn Fn(Option<String>) = &(|_: Option<String>| ());
        const FAIL: &dyn Fn(String, String) = &(|_: String, _: String| ());
        const SIGNER_FOR_SEAL: &dyn Fn() -> Option<Arc<dyn nmp_nip59::SignerForSeal>> =
            &(|| None);
        static EMPTY: crate::substrate::EmptyDmInboxRelayLookup =
            crate::substrate::EmptyDmInboxRelayLookup;
        let (command_sender, _rx) = std::sync::mpsc::channel::<ActorCommand>();
        Self {
            send,
            command_sender,
            now_secs: NOW,
            author_write_relays: RELAYS_BY_AUTHOR,
            bootstrap_discovery_relays: BOOTSTRAP,
            active_local_keys: LOCAL_KEYS,
            record_action_stage_requested: STAGE_REQ,
            nip17_local_keys: None,
            dm_inbox_relays: &EMPTY,
            set_last_error_toast: TOAST,
            record_action_failure: FAIL,
            signer_for_seal: SIGNER_FOR_SEAL,
            now_secs_snapshot: None,
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
        const TOAST: &dyn Fn(Option<String>) = &(|_: Option<String>| ());
        const FAIL: &dyn Fn(String, String) = &(|_: String, _: String| ());
        const SIGNER_FOR_SEAL: &dyn Fn() -> Option<Arc<dyn nmp_nip59::SignerForSeal>> =
            &(|| None);
        static EMPTY: crate::substrate::EmptyDmInboxRelayLookup =
            crate::substrate::EmptyDmInboxRelayLookup;
        Self {
            send,
            command_sender,
            now_secs: NOW,
            author_write_relays: RELAYS_BY_AUTHOR,
            bootstrap_discovery_relays: BOOTSTRAP,
            active_local_keys: LOCAL_KEYS,
            record_action_stage_requested: STAGE_REQ,
            nip17_local_keys: None,
            dm_inbox_relays: &EMPTY,
            set_last_error_toast: TOAST,
            record_action_failure: FAIL,
            signer_for_seal: SIGNER_FOR_SEAL,
            now_secs_snapshot: None,
        }
    }

    // V-39+V-40 builder-style setters. The production dispatch arm uses
    // the positional `new()` constructor; tests and a handful of
    // composition paths prefer the chained-builder shape.

    /// Builder — install the active account's local `nostr::Keys` for
    /// gift-wrap sealing. `None` keeps the default ("not signed in /
    /// remote signer"). The actor's dispatch arm reads this out of
    /// [`crate::ffi::Nip17LocalKeysSlot`] and passes it positionally to
    /// [`Self::new`]; tests use this builder atop [`Self::with_send_only`].
    #[must_use]
    pub fn with_nip17_local_keys(mut self, keys: Option<nostr::Keys>) -> Self {
        self.nip17_local_keys = keys;
        self
    }

    /// Builder — install the kernel-owned [`crate::substrate::DmInboxRelayLookup`]
    /// handle. Default is [`crate::substrate::EmptyDmInboxRelayLookup`] (every
    /// query returns `None` — the cold-start fail-closed contract).
    #[must_use]
    pub fn with_dm_inbox_relays(
        mut self,
        lookup: &'a dyn crate::substrate::DmInboxRelayLookup,
    ) -> Self {
        self.dm_inbox_relays = lookup;
        self
    }

    /// Builder — install a fixed wall-clock snapshot, overriding the
    /// `now_secs` closure for this context. The DM send path uses this in
    /// tests to drive `created_at` re-stamping against a deterministic
    /// clock without rebuilding the closure-shaped field. Production
    /// composition passes the kernel clock through the closure slot via
    /// [`Self::new`]; the override slot is left `None`.
    #[must_use]
    pub fn with_now_secs(mut self, now_secs: u64) -> Self {
        self.now_secs_snapshot = Some(now_secs);
        self
    }

    /// Builder — install a toast writer (the actor's dispatch arm wires
    /// this to `Kernel::set_last_error_toast`). D6 observable error.
    #[must_use]
    pub fn with_set_last_error_toast(
        mut self,
        f: &'a dyn Fn(Option<String>),
    ) -> Self {
        self.set_last_error_toast = f;
        self
    }

    /// Builder — install the action-failure recorder (the dispatch arm
    /// wires this to `Kernel::record_action_failure`). Used to close out
    /// the host spinner on an early-exit failure branch.
    #[must_use]
    pub fn with_record_action_failure(
        mut self,
        f: &'a dyn Fn(String, String),
    ) -> Self {
        self.record_action_failure = f;
        self
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
    /// commands MUST NOT call `SystemTime::now` directly).
    ///
    /// If [`Self::with_now_secs`] installed a snapshot override, returns
    /// it verbatim. Otherwise calls the closure installed by [`Self::new`]
    /// (which reads through the kernel). Returns `0` on a panicking
    /// closure (defensive — matches the `created_at = 0` sentinel the
    /// publish path already understands as "not stamped").
    pub fn now_secs(&self) -> u64 {
        if let Some(snap) = self.now_secs_snapshot {
            return snap;
        }
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

    /// V-39 — active account's local `nostr::Keys`, or `None` for a
    /// remote-signer account / not signed in. NIP-17 DM send uses these
    /// as a `SignerForSeal`; future protocols may consume them differently.
    /// Returns the stored value by reference — `nostr::Keys` is `Clone`
    /// and zeroizes its secret on drop, so the borrow does not leak
    /// across the command body.
    ///
    /// Distinct from [`Self::active_local_keys`]: the latter routes
    /// through a closure (V-41 surface; the dispatch arm reads through
    /// the identity runtime on every call), while this accessor returns
    /// the snapshot installed at context-construction time (the V-39
    /// surface — the dispatch arm reads the slot once and stores the
    /// `Option<Keys>`). Both observe the same value in production;
    /// commands choose whichever shape matches their needs.
    #[must_use]
    pub fn nip17_local_keys(&self) -> Option<&nostr::Keys> {
        self.nip17_local_keys.as_ref()
    }

    /// V-40 — DM-inbox relays for `pubkey`. `None` when no list is known
    /// (the gift-wrap publish path fails closed on `None` per NIP-17 §2;
    /// see [`crate::substrate::DmInboxRelayLookup`]).
    #[must_use]
    pub fn dm_inbox_relays(&self, pubkey: &str) -> Option<Vec<String>> {
        self.dm_inbox_relays.dm_inbox_relays(pubkey)
    }

    /// V-39 — set the kernel's last-error toast (D6 observable). `None`
    /// clears the toast. Wrapped in `catch_unwind` for D15 — see
    /// [`Self::send`] for the same reasoning.
    pub fn set_last_error_toast(&self, toast: Option<String>) {
        let f = self.set_last_error_toast;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(toast)));
    }

    /// V-39 — record a `Failed` terminal stage for `correlation_id` with
    /// `error` as the failure message. Used by the DM send path's
    /// early-exit branches to close the host spinner. Wrapped in
    /// `catch_unwind` for D15.
    pub fn record_action_failure(&self, correlation_id: String, error: String) {
        let f = self.record_action_failure;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            f(correlation_id, error);
        }));
    }

    /// Builder — install a [`SignerForSeal`][nmp_nip59::SignerForSeal]
    /// resolver. The DM send path uses this on every account flavour
    /// (local nsec via the blanket impl on `nostr::Keys`; NIP-46 bunker
    /// via the `RemoteSignerForSeal` adapter the actor wires through
    /// `IdentityRuntime::active_signer_for_seal`). Tests stage one of
    /// these to drive the bunker path through `with_send_only`; the
    /// production dispatch arm passes the closure positionally to
    /// [`Self::new`].
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn with_signer_for_seal(
        mut self,
        f: &'a dyn Fn() -> Option<Arc<dyn nmp_nip59::SignerForSeal>>,
    ) -> Self {
        self.signer_for_seal = f;
        self
    }

    /// V-08 (bunker DM send) — resolve a `SignerForSeal` for the active
    /// account. Returns:
    ///
    /// - `Some(Arc<dyn SignerForSeal>)` for a **local** account — the
    ///   blanket impl on `nostr::Keys` resolves every chain step
    ///   synchronously, so the seal runs on the actor thread.
    /// - `Some(Arc<dyn SignerForSeal>)` for a **remote (NIP-46 bunker)**
    ///   account — `RemoteSignerForSeal` adapts the active
    ///   `RemoteSignerHandle`; `gift_wrap_with_signer` spawns a per-
    ///   invocation driver thread for the `Pending` chain so the actor
    ///   itself does not block on bunker RPCs.
    /// - `None` when no account is active OR a remote signer reports a
    ///   malformed pubkey (graceful-degrade — the DM send path surfaces
    ///   a toast).
    ///
    /// Wrapped in `catch_unwind` for D15 (panicking accessor falls back
    /// to `None`, matching the genuinely-absent-signer branch).
    ///
    /// This replaces the V-39 [`Self::nip17_local_keys`] read on the
    /// NIP-17 DM send path: the legacy accessor returned `None` for
    /// bunker accounts and is kept only so existing callers (none on the
    /// seal path after V-08) compile unchanged.
    #[must_use]
    pub fn signer_for_seal(&self) -> Option<Arc<dyn nmp_nip59::SignerForSeal>> {
        let f = self.signer_for_seal;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f())).unwrap_or(None)
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
        // V-39+V-40 surface defaults are also safe.
        assert!(ctx.nip17_local_keys().is_none());
        assert!(ctx.dm_inbox_relays("anything").is_none());
        ctx.set_last_error_toast(Some("toast".to_string()));
        ctx.record_action_failure("cid".to_string(), "err".to_string());
        // V-08 surface default — no active account ⇒ no signer.
        assert!(ctx.signer_for_seal().is_none());
    }

    // The full constructor threads its closures through unchanged — the
    // V-41 dispatch arm relies on this directly. V-39+V-40 added four
    // trailing positional args (nip17 keys, DM-inbox lookup, toast,
    // record-failure); this test exercises those too.
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
        let toast_seen: Mutex<Vec<Option<String>>> = Mutex::new(Vec::new());
        let toast = |t: Option<String>| toast_seen.lock().unwrap().push(t);
        let fail_seen: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
        let fail = |cid: String, err: String| {
            fail_seen.lock().unwrap().push((cid, err));
        };
        let dm_empty = crate::substrate::EmptyDmInboxRelayLookup;
        let (tx, rx) = mpsc::channel::<ActorCommand>();
        // V-08: the signer-for-seal closure resolves to the active account's
        // `SignerForSeal`; the no-active-account branch returns `None`
        // (covered here so the positional shape stays exercised). End-to-end
        // bunker coverage lives in the `remote_signer_tests` module.
        let signer_for_seal = || None::<Arc<dyn nmp_nip59::SignerForSeal>>;

        let ctx = ProtocolCommandContext::new(
            &send,
            tx,
            &now_secs,
            &by_author,
            &stub_relays,
            &local_keys,
            &stage_req,
            None,
            &dm_empty,
            &toast,
            &fail,
            &signer_for_seal,
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
        // V-39+V-40 — the new positional args reach their fields.
        assert!(ctx.nip17_local_keys().is_none());
        assert!(ctx.dm_inbox_relays("anyone").is_none());
        ctx.set_last_error_toast(Some("hello".to_string()));
        ctx.record_action_failure("cid-z".to_string(), "boom".to_string());
        // V-08 — the signer-for-seal closure reaches its field too.
        assert!(ctx.signer_for_seal().is_none());
        assert_eq!(*toast_seen.lock().unwrap(), vec![Some("hello".to_string())]);
        assert_eq!(
            *fail_seen.lock().unwrap(),
            vec![("cid-z".to_string(), "boom".to_string())]
        );
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

    // V-39+V-40 — builder methods overlay onto the with_send_only default
    // and are honoured by the matching accessors.
    #[test]
    fn builder_methods_override_defaults() {
        let send = |_: ActorCommand| {};
        let toasts: Mutex<Vec<Option<String>>> = Mutex::new(Vec::new());
        let fails: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
        let toast = |t: Option<String>| toasts.lock().unwrap().push(t);
        let fail = |cid: String, err: String| fails.lock().unwrap().push((cid, err));
        let dm = crate::substrate::TestDmInboxRelayCache::new();
        dm.upsert(
            "pk1",
            &["wss://dm.example"],
        );
        let keys = nostr::Keys::generate();
        let seal_keys = keys.clone();
        let signer_for_seal = move || {
            Some(Arc::new(seal_keys.clone()) as Arc<dyn nmp_nip59::SignerForSeal>)
        };
        let ctx = ProtocolCommandContext::with_send_only(&send)
            .with_nip17_local_keys(Some(keys.clone()))
            .with_dm_inbox_relays(&dm)
            .with_now_secs(987_654)
            .with_set_last_error_toast(&toast)
            .with_record_action_failure(&fail)
            .with_signer_for_seal(&signer_for_seal);
        assert_eq!(ctx.now_secs(), 987_654);
        assert!(ctx.nip17_local_keys().is_some());
        assert!(
            ctx.signer_for_seal().is_some(),
            "with_signer_for_seal overrides the noop default"
        );
        assert_eq!(
            ctx.dm_inbox_relays("pk1").as_deref(),
            Some(&["wss://dm.example".to_string()][..])
        );
        ctx.set_last_error_toast(Some("hi".to_string()));
        ctx.record_action_failure("c".to_string(), "e".to_string());
        assert_eq!(*toasts.lock().unwrap(), vec![Some("hi".to_string())]);
        assert_eq!(
            *fails.lock().unwrap(),
            vec![("c".to_string(), "e".to_string())]
        );
    }
}
