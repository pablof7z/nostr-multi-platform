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
    /// bunker accounts (which sign through the actor's signer port) and when
    /// no account is active.
    fn active_local_keys(&self) -> Option<nostr::Keys>;

    /// Active account's hex pubkey, backend-transparent (local nsec OR remote
    /// signer). `None` when no account is active.
    ///
    /// ADR-0050 §D5 — the gift-wrap DM chain resolves the active account's
    /// pubkey ONCE at step 1 through this accessor and pins every subsequent
    /// port step with `signer_pubkey: Some(hex)`, so a mid-chain account switch
    /// signs the seal with the originating account. Replaces `signer_for_seal`.
    fn active_account_pubkey(&self) -> Option<String>;
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
    fn active_account_pubkey(&self) -> Option<String> {
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

#[path = "protocol/context.rs"]
mod context_impl;

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

    /// Downcast hook for instance-scoping verification (ADR-0052 D1/D2).
    ///
    /// A `ProtocolCommand` captures its dependencies by value at composition
    /// time (e.g. a NIP-47 `WalletRuntimeHandle`). To assert — in a test —
    /// that a dispatched command carries the *correct per-app* dependency
    /// rather than a shared process-global, the boxed trait object must be
    /// downcastable to its concrete type. This is the standard object-safe
    /// `Any` upcast; production dispatch never calls it.
    ///
    /// Implementors return `self`; the one-line body cannot be a trait
    /// default because coercing `&Self` to `&dyn Any` requires `Self: Sized`,
    /// which a trait-object default body does not have.
    fn as_any(&self) -> &dyn core::any::Any;
}

#[path = "protocol/builders.rs"]
mod builders;
pub use builders::{
    build_nip44_decrypt_for_account, build_nip44_encrypt_for_account, build_sign_event_for_account,
};

#[cfg(test)]
#[path = "protocol/tests.rs"]
mod tests;
