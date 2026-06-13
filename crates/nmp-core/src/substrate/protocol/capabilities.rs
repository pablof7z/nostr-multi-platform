//! Capability traits + their noop impls for the `ProtocolCommand` seam.
//!
//! Split out of `substrate/protocol.rs` (file-size discipline) — these are the
//! typed capability surfaces bundled by `ProtocolCommandContextParts` and
//! exposed through `ProtocolCommandContext`. They are re-exported from
//! `protocol.rs` so the `crate::substrate::*` public paths are unchanged.
//!
//! Debt C — these traits replaced a 12-positional-closure bundle: each one
//! names the surface a given `ctx.*()` call belongs to. The `Noop*` singletons
//! are the `with_send_only` defaults and the fall-throughs for NIP-crate tests
//! that don't exercise a given surface.

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

/// ADR-0052 §D4 — the narrow capability that reaches the per-app
/// [`HostOpHandler`](crate::substrate::HostOpHandler) slot.
///
/// This is the seam K2 rung 5.4 adds so the persistent, host-installed
/// handler (the Marmot MLS service, hot-swappable on account switch) can be
/// expressed as a one-shot [`ProtocolCommand`](super::ProtocolCommand) (the
/// `HostOpCommand` in [`crate::substrate::host_op`]) instead of a bespoke
/// `ActorCommand::DispatchHostOp` arm. The command captures NO handler itself
/// — it asks this capability for an `Arc::clone` of whatever handler is
/// installed *now*, so account-switch hot-swaps stay live (D2: the value the
/// command reaches is per-app slot state, not baked into the command).
///
/// It is deliberately the only door the command has onto app/actor state — it
/// does NOT hand out `&mut Kernel` (rung 5.5 owns `kernel_mut`); it returns
/// only the opaque `Arc<dyn HostOpHandler>` (D0: no protocol type crosses).
pub trait HostOpHandlerAccess: Send + Sync {
    /// Clone the currently-installed handler out of the per-app slot, or
    /// `None` if no handler was installed before the dispatch reached the
    /// actor. The clone is taken under the slot lock and returned by value so
    /// the long-running `handle` call never holds the slot mutex (D8 — must
    /// not block the FFI `set_host_op_handler` writer).
    fn current_handler(&self) -> Option<std::sync::Arc<dyn crate::substrate::HostOpHandler>>;
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
/// the `with_send_only` default and NIP crate tests that don't exercise the
/// routing surface install this singleton.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopRecipientRelayLookup;

impl RecipientRelayLookup for NoopRecipientRelayLookup {
    fn recipient_publish_relays(&self, _recipient: &str, _kind: u32) -> Vec<String> {
        Vec::new()
    }
}

/// Noop [`HostOpHandlerAccess`] — always reports no installed handler.
/// Mirrors the "no stateful app bound" branch (the test / no-handler default).
/// Installed by `with_send_only` and by NIP crate tests that never exercise
/// the host-op seam.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopHostOpHandlerAccess;

impl HostOpHandlerAccess for NoopHostOpHandlerAccess {
    fn current_handler(&self) -> Option<std::sync::Arc<dyn crate::substrate::HostOpHandler>> {
        None
    }
}
