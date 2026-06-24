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
pub trait KernelClock {
    /// Seconds since the Unix epoch.
    fn now_secs(&self) -> u64;
}

/// Active-account local signing material. Used by NIP commands that need
/// to mint a signature on the actor thread (NIP-57 kind:9734 signing,
/// NIP-17 gift-wrap sealing).
pub trait LocalSignerAccess {
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
pub trait ErrorSurface {
    /// Write the `last_error_toast` projection. `None` clears the toast.
    fn set_last_error_toast(&self, message: Option<String>);

    /// Write a structured [`UiToken`](crate::ui_token::UiToken) (issue #1682):
    /// the stable machine `code` to `last_error_category` and the English
    /// `fallback_prose` to `last_error_toast`, logging `raw_detail`. The shell
    /// renders localized prose from the code. The default impl degrades to the
    /// fallback prose only (no category) for non-kernel-backed implementations
    /// (tests / no-op surfaces); the production adapter overrides it.
    fn set_last_error_token(&self, token: &crate::ui_token::UiToken) {
        self.set_last_error_toast(Some(token.fallback_prose().to_string()));
    }

    /// Record a `Failed` terminal stage for `correlation_id` with
    /// `reason` as the failure message.
    fn record_action_failure(&self, correlation_id: String, reason: String);
}

/// Action-stage write surface — the `Requested` transition recorded
/// against an in-flight `correlation_id`. Idempotent.
pub trait ActionStageTracker {
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
pub trait RecipientRelayLookup {
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

/// ADR-0052 §D5 — the narrow kernel surface the NIP-47 wallet runtime
/// mutates on the actor thread, promoted off the deleted `kernel_mut()`
/// escape hatch.
///
/// Before rung 5.5 the three wallet `ProtocolCommand`s reached the whole
/// `&mut Kernel` through `ProtocolCommandContext::kernel_mut()` — ambient
/// authority that defeated the narrow capability traits the context already
/// offered. The wallet runtime helpers (`nmp_nip47::runtime`) touch exactly
/// eight kernel methods; this trait is that closed set and nothing more, so a
/// wallet command can no longer reach (e.g.) the event store, the planner, or
/// the outbox router. Each method is `&self` with interior mutability in the
/// adapter (`try_borrow_mut` on the dispatch arm's `RefCell<&mut Kernel>`),
/// mirroring [`ErrorSurface`].
///
/// D0: every method names only protocol-neutral kernel primitives
/// ([`RelayRole`](nmp_network::role::RelayRole), [`AuthSignerFn`](crate::AuthSignerFn),
/// persistent-sub ids) — no NIP-47 / wallet protocol concept crosses into
/// `nmp-core`. The trait is consumed identically by the actor's
/// `RelayTextInterceptor` path (which holds a real `&mut Kernel`) via
/// [`Kernel::as_wallet_access`](crate::Kernel::as_wallet_access).
pub trait WalletKernelAccess {
    /// Wall-clock seconds since the Unix epoch (kernel-owned clock; D7).
    fn now_secs(&self) -> u64;

    /// Write the `last_error_toast` projection. `None` clears the toast.
    fn set_last_error_toast(&self, message: Option<String>);

    /// Write a structured [`UiToken`](crate::ui_token::UiToken) (issue #1682):
    /// machine `code` to `last_error_category`, English `fallback_prose` to
    /// `last_error_toast`, logging `raw_detail`. The default impl degrades to
    /// the fallback prose only; the production adapter overrides it.
    fn set_last_error_token(&self, token: &crate::ui_token::UiToken) {
        self.set_last_error_toast(Some(token.fallback_prose().to_string()));
    }

    /// Record a `Failed` terminal stage for `correlation_id` with `reason`.
    fn record_action_failure(&self, correlation_id: String, reason: String);

    /// Record an `Accepted` terminal stage for `correlation_id` carrying the
    /// optional `result_json` payload (a settled `pay_invoice` preimage).
    fn record_action_success(&self, correlation_id: String, result_json: Option<String>);

    /// Bind the per-role NIP-42 auth signer (the NWC client secret signs the
    /// `RelayRole::Wallet` lane).
    fn set_relay_auth_signer(
        &self,
        role: nmp_network::role::RelayRole,
        pubkey_hex: String,
        signer: crate::AuthSignerFn,
    );

    /// Drop the signer for `role` (wallet disconnect clears the wallet lane).
    fn clear_relay_auth_signer(&self, role: nmp_network::role::RelayRole);

    /// Register `(relay_url, sub_id)` as persistent so EOSE does not auto-CLOSE
    /// the long-lived kind:23195 listener.
    fn register_persistent_sub(&self, relay_url: String, sub_id: String);

    /// Remove `(relay_url, sub_id)` from the persistent set (disconnect).
    fn unregister_persistent_sub(&self, relay_url: &str, sub_id: &str);

    /// Mark the snapshot dirty so the next tick carries the wallet status write.
    fn mark_changed_since_emit(&self);
}

/// ADR-0052 §D5 — the zap-specific cached-profile read, promoted off the
/// generic [`ProtocolCommandContext`](super::ProtocolCommandContext) (where it
/// was `lnurl_for_pubkey`, visible to *every* command) onto a dedicated
/// capability only the NIP-57 zap path reaches.
///
/// Resolves the recipient's lightning address / LNURL from the kernel's cached
/// kind:0 profile. The data lives in the kernel (it arrives at runtime), so the
/// command cannot capture it at composition time the way it captures its wallet
/// handle — it is a narrow read capability instead (the [`RecipientRelayLookup`]
/// shape). D0: returns a bare `Option<String>`; no zap/NIP-57 type crosses.
pub trait ZapProfileLookup {
    /// The recipient's lightning address / LNURL from their cached kind:0
    /// profile, or `None` when the profile has not arrived yet or carries no
    /// lightning address.
    fn lnurl_for_pubkey(&self, pubkey: &str) -> Option<String>;
}

/// ADR-0052 §D4 — the narrow capability that reaches the actor's configured
/// [`HostOpHandler`](crate::substrate::HostOpHandler).
///
/// This is the seam K2 rung 5.4 adds so the persistent, host-installed handler
/// (the Marmot MLS service) can be expressed as a one-shot
/// [`ProtocolCommand`](super::ProtocolCommand) (the `HostOpCommand` in
/// [`crate::substrate::host_op`]) instead of a bespoke
/// `ActorCommand::DispatchHostOp` arm. The command captures NO handler itself:
/// it asks this capability for an `Arc::clone` of the handler that was snapped
/// into actor config at start.
///
/// It is deliberately narrow — it does NOT hand out `&mut Kernel` (rung 5.5
/// deleted that escape hatch entirely); it returns only the opaque
/// `Arc<dyn HostOpHandler>` (D0: no protocol type crosses).
pub trait HostOpHandlerAccess {
    /// Clone the configured handler, or `None` if no handler was installed
    /// before actor start. The clone is returned by value so the long-running
    /// `handle` call never depends on shared config locks.
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

/// Noop [`WalletKernelAccess`] — discards every wallet kernel mutation and
/// returns `0` for the clock. Mirrors the "no kernel attached" branch (the
/// `with_send_only` default and NIP crate tests that never drive the wallet
/// runtime against a kernel).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopWalletKernelAccess;

impl WalletKernelAccess for NoopWalletKernelAccess {
    fn now_secs(&self) -> u64 {
        0
    }
    fn set_last_error_toast(&self, _message: Option<String>) {}
    fn record_action_failure(&self, _correlation_id: String, _reason: String) {}
    fn record_action_success(&self, _correlation_id: String, _result_json: Option<String>) {}
    fn set_relay_auth_signer(
        &self,
        _role: nmp_network::role::RelayRole,
        _pubkey_hex: String,
        _signer: crate::AuthSignerFn,
    ) {
    }
    fn clear_relay_auth_signer(&self, _role: nmp_network::role::RelayRole) {}
    fn register_persistent_sub(&self, _relay_url: String, _sub_id: String) {}
    fn unregister_persistent_sub(&self, _relay_url: &str, _sub_id: &str) {}
    fn mark_changed_since_emit(&self) {}
}

/// Noop [`ZapProfileLookup`] — always reports no cached lightning address.
/// Mirrors the "profile not arrived" branch; the `with_send_only` default and
/// NIP crate tests that don't exercise the zap-resolve surface install this.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopZapProfileLookup;

impl ZapProfileLookup for NoopZapProfileLookup {
    fn lnurl_for_pubkey(&self, _pubkey: &str) -> Option<String> {
        None
    }
}
