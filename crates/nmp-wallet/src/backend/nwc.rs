//! NWC (`nmp-nip47`) [`WalletBackend`] adapter — the first concrete backend
//! behind the seam (#2886, epic #2864).
//!
//! This adapter does not open relay sockets, decrypt NIP-04/44 payloads, or
//! own the `pay_invoice`/`connect`/`disconnect` `ActionModule`s — those stay
//! exactly where `docs/architecture/nip60-nip61-wallet-design.md` puts them,
//! in `nmp-nip47`. What this file owns is the narrow translation from the
//! generic [`WalletBackend`] seam to `nmp-nip47`'s existing composition
//! surface (`WalletRuntimeHandle`, `WalletPayInvoiceCommand`, `WalletStatus`):
//!
//! * `capabilities()` advertises exactly `pay_bolt11` — NWC has no Cashu/
//!   nutzap capability, and absent capability means absent user action
//!   (`WalletCapabilities::action_namespaces`), not a runtime failure.
//! * `start_intent()` maps `WalletIntent::PayBolt11` onto the same
//!   `ActorCommand::Protocol(WalletPayInvoiceCommand)` the
//!   `nmp.wallet.pay_invoice` `ActionModule` already emits. Every other
//!   intent variant is Cashu/nutzap-shaped and NWC does not implement it, so
//!   `start_intent` is a documented no-op for those (never a panic — D6).
//! * `snapshot()` derives `WalletReadiness` from the shared `WalletStatus`
//!   slot `nmp-nip47`'s runtime is the sole writer of (D4). It does **not**
//!   populate `WalletProjection::balances`: `WalletBalanceRow` is a per-mint
//!   Cashu row ("balances by unit and mint, aggregated without proofs" —
//!   nip60-nip61-wallet-design.md), and a single-purse Lightning balance has
//!   no mint analog. Surfacing `WalletStatus::balance_sats` through the
//!   bounded wallet projection is a projection-shape decision left to
//!   whoever extends that shape next, not invented here.
//!
//! # NOT wired here (escalated, not silently skipped)
//!
//! `on_wallet_event` and `on_mint_result` are intentionally no-ops:
//!
//! * kind:23195 NWC response reconciliation already flows through
//!   `nmp-nip47`'s own `WalletInterceptor` (a `RelayTextInterceptor`
//!   registered by `nmp_nip47::register`), which decrypts the raw relay
//!   frame text against the connection's NIP-04/44 secret and returns
//!   `Vec<OutboundMessage>`. This seam's `on_wallet_event` receives an
//!   already-parsed [`KernelEvent`] (id/author/kind/tags/content, no
//!   decryption key) and must return `Vec<ActorCommand>` — a different input
//!   shape and a different output type. Bridging the two (or replacing one
//!   with the other) is actor-wiring/registration work, not adapter work; it
//!   is out of this ticket's scope.
//! * `MintResult` is a Cashu mint-quote-monitoring concept; NWC has no mint
//!   interactions, so there is nothing for this backend to do with it.
//!
//! Constructing a live [`NwcWalletBackend`] additionally needs a
//! [`WalletStatusSlot`] clone bound to the same runtime `nmp_nip47::register`
//! wires up — `register`'s returned `Handles` now exposes one as
//! `Handles::status` (#2894). Threading that clone into a live
//! `NwcWalletBackend` end to end is still composition-root work, not this
//! adapter's.

use nmp_nip47::{
    NwcConnectionState, WalletPayInvoiceCommand, WalletRuntimeHandle, WalletStatus,
    WalletStatusSlot,
};

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::KernelEvent;

use crate::backend::{
    MintResult, WalletBackend, WalletBackendContext, WalletBackendId, WalletBackendSnapshot,
    WalletIntent, WalletProjectionScope,
};
use crate::capability::WalletCapabilities;
use crate::projection::{WalletProjection, WalletReadiness};

/// Canonical id this backend registers under.
pub const NWC_BACKEND_ID: &str = "nwc";

/// [`WalletBackend`] adapter over `nmp-nip47`'s NWC runtime.
pub struct NwcWalletBackend {
    runtime: WalletRuntimeHandle,
    status: WalletStatusSlot,
}

impl NwcWalletBackend {
    /// Construct the adapter bound to the per-app `WalletRuntimeHandle` (also
    /// held by `nmp-nip47`'s connect/disconnect/pay_invoice `ActionModule`s)
    /// and the `WalletStatusSlot` its runtime writes on every connection
    /// state change (D4: the runtime is the sole writer).
    #[must_use]
    pub fn new(runtime: WalletRuntimeHandle, status: WalletStatusSlot) -> Self {
        Self { runtime, status }
    }

    /// Discard the identity-scoped NWC connection + status on a Nostr account
    /// switch, mirroring `CashuWalletBackend::reset()` (#2916).
    ///
    /// The owner's settled decision: an NWC Lightning connection is
    /// Nostr-account-scoped, not account-independent — it resets on account
    /// switch exactly as Cashu does. `register.rs` wires this to the same
    /// `IdentityChangeRegistrar` signal both backends now reset on, keeping the
    /// merged `"wallet"` projection consistent (previously Cashu reset while
    /// NWC's previous-account connection/status was retained).
    ///
    /// Delegates to the actor-side [`WalletRuntime::reset`] behind the shared
    /// handle (which drops the `WalletConnection` — connection URI, in-flight
    /// payment tracking — and clears the status slot both backends' snapshots
    /// read). It ALSO clears the status slot directly: the handle may hold no
    /// runtime yet (nothing seeded), in which case a status could still have
    /// been written; this is idempotent when the runtime already cleared its
    /// own clone of the same `Arc`.
    pub fn reset(&self) {
        // Clear the actor-side runtime connection if one has been seeded into
        // the handle. Poison is recovered, not fatal (D6) — the whole point is
        // to end up with no connection regardless.
        match self.runtime.lock() {
            Ok(mut guard) => {
                if let Some(runtime) = guard.as_mut() {
                    runtime.reset();
                }
            }
            Err(poisoned) => {
                if let Some(runtime) = poisoned.into_inner().as_mut() {
                    runtime.reset();
                }
            }
        }
        // Defensively clear the shared status slot (the same `Arc` the runtime
        // writes, cloned into this backend at construction).
        match self.status.lock() {
            Ok(mut guard) => *guard = None,
            Err(poisoned) => *poisoned.into_inner() = None,
        }
    }

    /// Reads the shared status slot. A poisoned mutex is recovered rather
    /// than collapsed to "no status" — the last-written `WalletStatus` is
    /// still the best information available, and reporting `NotConfigured`
    /// on a poison would misrepresent an active-but-degraded connection as
    /// never having been configured at all (D6: poison is not fatal, and we
    /// must not lie about what was last written — same principle
    /// `sync_wallet_status`/`disconnect` apply on the write side in
    /// `nmp-nip47`).
    fn current_status(&self) -> Option<WalletStatus> {
        match self.status.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}

impl WalletBackend for NwcWalletBackend {
    fn id(&self) -> WalletBackendId {
        WalletBackendId::new(NWC_BACKEND_ID)
    }

    fn capabilities(&self) -> WalletCapabilities {
        WalletCapabilities::nwc_payments()
    }

    fn snapshot(&self, _scope: WalletProjectionScope) -> WalletBackendSnapshot {
        let readiness = readiness_from_status(self.current_status().as_ref());
        WalletBackendSnapshot {
            projection: WalletProjection::new(Some(self.id()), readiness, self.capabilities()),
        }
    }

    /// Translates an already-validated [`WalletIntent`] into commands.
    ///
    /// Like `ActionModule::execute` (as opposed to `ActionModule::start`),
    /// this method does not itself validate or dedupe — it trusts the
    /// caller. `nmp-nip47`'s own `nmp.wallet.pay_invoice` `ActionModule`
    /// rejects an empty `bolt11` and a same-invoice retap in `start()` before
    /// `execute()` ever runs; whichever `nmp-wallet` dispatch path calls this
    /// seam for a selected backend must apply the equivalent gate before
    /// calling `start_intent`, or a bad/duplicate `PayBolt11` intent reaches
    /// the wallet runtime unfiltered.
    fn start_intent(
        &self,
        _ctx: WalletBackendContext<'_>,
        intent: WalletIntent,
        correlation_id: Option<String>,
    ) -> Vec<ActorCommand> {
        match intent {
            WalletIntent::PayBolt11 {
                bolt11,
                amount_msats,
            } => vec![ActorCommand::Protocol(Box::new(WalletPayInvoiceCommand {
                bolt11,
                amount_msats,
                correlation_id,
                runtime: self.runtime.clone(),
            }))],
            // Cashu/nutzap intents and backend-selection are not NWC's
            // capability — `capabilities()` already tells callers not to
            // route these here. A no-op rather than a panic keeps a stray
            // dispatch harmless (D6) instead of surprising.
            WalletIntent::SelectBackend { .. }
            | WalletIntent::CreateCashuWallet { .. }
            | WalletIntent::RecoverCashuWallet
            | WalletIntent::SetCashuMints { .. }
            | WalletIntent::PublishNutzapInfo
            | WalletIntent::SendNutzap { .. }
            | WalletIntent::RedeemNutzap { .. }
            | WalletIntent::DepositQuoteCashu { .. }
            | WalletIntent::CompleteDepositCashu { .. }
            | WalletIntent::MeltCashu { .. } => Vec::new(),
        }
    }

    fn on_wallet_event(
        &self,
        _ctx: WalletBackendContext<'_>,
        _event: &KernelEvent,
    ) -> Vec<ActorCommand> {
        // See the module doc comment: kind:23195 reconciliation already runs
        // through `nmp-nip47`'s `WalletInterceptor`, which needs the raw
        // relay frame text and the connection's decryption key — neither of
        // which this `KernelEvent`-shaped seam carries. Not wired here.
        Vec::new()
    }

    fn on_mint_result(
        &self,
        _ctx: WalletBackendContext<'_>,
        _result: MintResult,
    ) -> Vec<ActorCommand> {
        // Cashu-only concept; NWC has no mint interactions.
        Vec::new()
    }
}

/// Map `nmp-nip47`'s raw `WalletStatus` token (plus the V-79 heartbeat-derived
/// `connection_state`) onto the seam's coarse [`WalletReadiness`].
///
/// `status.rs` documents `status == "ready"` as reflecting the last *protocol*
/// state, not real-time relay reachability — the heartbeat-derived
/// `connection_state` is the more current signal once it exists, so it is
/// checked FIRST and wins over the raw token regardless of which token the
/// connection is sitting at. This matters because the heartbeat loop starts
/// ticking as soon as a connection exists, which is before the first
/// kind:23195 response ever lands (`status` stays `"connecting"` until
/// then) — so `Reconnecting`/`TransportLost` can be observed while `status`
/// is still `"connecting"`, not only once it reaches `"ready"`.
fn readiness_from_status(status: Option<&WalletStatus>) -> WalletReadiness {
    let Some(status) = status else {
        return WalletReadiness::NotConfigured;
    };
    match status.connection_state {
        Some(NwcConnectionState::TransportLost) => return WalletReadiness::Degraded,
        Some(NwcConnectionState::Reconnecting) => return WalletReadiness::Activating,
        Some(NwcConnectionState::Connected) | None => {}
    }
    match status.status.as_str() {
        "connecting" => WalletReadiness::Activating,
        "ready" => WalletReadiness::Ready,
        "error" => WalletReadiness::Degraded,
        "disconnected" => WalletReadiness::NotConfigured,
        // Unknown status token: fail toward visibly-broken, not falsely-ready.
        _ => WalletReadiness::Degraded,
    }
}

#[cfg(test)]
#[path = "nwc_tests.rs"]
mod tests;
