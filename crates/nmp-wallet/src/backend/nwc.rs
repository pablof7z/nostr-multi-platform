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
//! wires up — `register`'s returned `Handles` does not expose one today (only
//! `Handles::wallet`). Wiring that through end to end is composition-root
//! work, not this adapter's.

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
            | WalletIntent::PublishNutzapInfo
            | WalletIntent::SendNutzap { .. }
            | WalletIntent::RedeemNutzap { .. }
            | WalletIntent::DepositCashu { .. }
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
mod tests {
    use super::*;

    fn backend() -> (NwcWalletBackend, WalletStatusSlot) {
        let runtime = nmp_nip47::new_wallet_runtime_handle();
        let status = nmp_nip47::new_wallet_status_slot();
        (NwcWalletBackend::new(runtime, status.clone()), status)
    }

    fn ctx() -> WalletBackendContext<'static> {
        WalletBackendContext {
            now_secs: 0,
            selected_backend: None,
        }
    }

    fn status(token: &str, connection_state: Option<NwcConnectionState>) -> WalletStatus {
        WalletStatus {
            status: token.to_string(),
            relay_url: "wss://relay.example.com".to_string(),
            wallet_pubkey_hex: "a".repeat(64),
            balance_msats: Some(21_000),
            balance_sats: Some(21),
            is_ready: token == "ready",
            is_connected: token == "ready" || token == "connecting",
            connection_state,
        }
    }

    #[test]
    fn advertises_only_pay_bolt11() {
        let (backend, _status) = backend();
        let caps = backend.capabilities();

        assert!(caps.pay_bolt11);
        assert!(!caps.create_cashu_wallet);
        assert!(!caps.publish_nutzap_info);
        assert!(!caps.send_nutzap);
        assert!(!caps.redeem_nutzap);
        assert!(!caps.deposit_cashu);
        assert!(!caps.melt_cashu);
        assert!(!caps.observe_nutzap_receipts);
    }

    #[test]
    fn pay_bolt11_intent_emits_one_protocol_command() {
        let (backend, _status) = backend();

        let commands = backend.start_intent(
            ctx(),
            WalletIntent::PayBolt11 {
                bolt11: "lnbc100n1p0fake".to_string(),
                amount_msats: Some(1_000),
            },
            Some("corr-1".to_string()),
        );

        assert_eq!(commands.len(), 1);
        assert!(matches!(commands[0], ActorCommand::Protocol(_)));

        // `WalletPayInvoiceCommand` is type-erased behind `Box<dyn
        // ProtocolCommand>`; there is no `Any` downcast on that trait
        // (matching `nmp-nip47`'s own `WalletPayInvoiceModule` tests). Its
        // `Debug` impl is a supertrait bound, though, so this checks that
        // `bolt11`/`amount_msats`/`correlation_id` were actually threaded
        // through rather than dropped or swapped.
        let debug = format!("{:?}", commands[0]);
        assert!(debug.contains("lnbc100n1p0fake"));
        assert!(debug.contains("1000"));
        assert!(debug.contains("corr-1"));
    }

    #[test]
    fn cashu_and_nutzap_intents_are_a_documented_no_op() {
        let (backend, _status) = backend();

        let unsupported = [
            WalletIntent::SelectBackend {
                backend_id: WalletBackendId::new(NWC_BACKEND_ID),
            },
            WalletIntent::CreateCashuWallet {
                mint: "https://mint.example.com".to_string(),
            },
            WalletIntent::RecoverCashuWallet,
            WalletIntent::PublishNutzapInfo,
            WalletIntent::SendNutzap {
                recipient_pubkey: "b".repeat(64),
                amount_sats: 21,
                target_event_id: None,
            },
            WalletIntent::RedeemNutzap {
                event_id: "c".repeat(64),
            },
            WalletIntent::DepositCashu { amount_sats: 21 },
            WalletIntent::MeltCashu {
                bolt11: "lnbc100n1p0fake".to_string(),
            },
        ];

        for intent in unsupported {
            assert!(backend.start_intent(ctx(), intent, None).is_empty());
        }
    }

    #[test]
    fn snapshot_reports_not_configured_before_any_status_is_written() {
        let (backend, _status) = backend();
        let snapshot = backend.snapshot(WalletProjectionScope::default());

        assert_eq!(
            snapshot.projection.readiness,
            WalletReadiness::NotConfigured
        );
        assert_eq!(
            snapshot
                .projection
                .active_backend_id
                .as_ref()
                .unwrap()
                .as_str(),
            NWC_BACKEND_ID
        );
        assert!(snapshot.projection.balances.is_empty());
    }

    #[test]
    fn snapshot_tracks_connecting_ready_and_error_status_tokens() {
        let (backend, status_slot) = backend();

        *status_slot.lock().unwrap() = Some(status("connecting", None));
        assert_eq!(
            backend
                .snapshot(WalletProjectionScope::default())
                .projection
                .readiness,
            WalletReadiness::Activating
        );

        *status_slot.lock().unwrap() = Some(status("ready", None));
        assert_eq!(
            backend
                .snapshot(WalletProjectionScope::default())
                .projection
                .readiness,
            WalletReadiness::Ready
        );

        *status_slot.lock().unwrap() = Some(status("error", None));
        assert_eq!(
            backend
                .snapshot(WalletProjectionScope::default())
                .projection
                .readiness,
            WalletReadiness::Degraded
        );

        *status_slot.lock().unwrap() = Some(status("disconnected", None));
        assert_eq!(
            backend
                .snapshot(WalletProjectionScope::default())
                .projection
                .readiness,
            WalletReadiness::NotConfigured
        );
    }

    #[test]
    fn heartbeat_connection_state_overrides_a_stale_ready_token() {
        let (backend, status_slot) = backend();

        *status_slot.lock().unwrap() =
            Some(status("ready", Some(NwcConnectionState::TransportLost)));
        assert_eq!(
            backend
                .snapshot(WalletProjectionScope::default())
                .projection
                .readiness,
            WalletReadiness::Degraded,
            "a live transport-lost signal must win over a stale ready token"
        );

        *status_slot.lock().unwrap() =
            Some(status("ready", Some(NwcConnectionState::Reconnecting)));
        assert_eq!(
            backend
                .snapshot(WalletProjectionScope::default())
                .projection
                .readiness,
            WalletReadiness::Activating
        );

        *status_slot.lock().unwrap() = Some(status("ready", Some(NwcConnectionState::Connected)));
        assert_eq!(
            backend
                .snapshot(WalletProjectionScope::default())
                .projection
                .readiness,
            WalletReadiness::Ready
        );
    }

    /// The heartbeat loop ticks as soon as a connection exists — before the
    /// first kind:23195 response ever lands, i.e. while `status` is still
    /// `"connecting"` (see `WalletConnection::status` in
    /// `nmp-nip47/src/runtime/commands.rs`). A transport failure observed
    /// during that window must still surface as `Degraded`/`Activating`, not
    /// get stuck reporting `Activating` forever because the token-only match
    /// only ever looked at `connection_state` under the `"ready"` arm.
    #[test]
    fn heartbeat_connection_state_overrides_a_still_connecting_token() {
        let (backend, status_slot) = backend();

        *status_slot.lock().unwrap() = Some(status(
            "connecting",
            Some(NwcConnectionState::TransportLost),
        ));
        assert_eq!(
            backend
                .snapshot(WalletProjectionScope::default())
                .projection
                .readiness,
            WalletReadiness::Degraded,
            "transport-lost must win even if the first response never arrived"
        );

        *status_slot.lock().unwrap() =
            Some(status("connecting", Some(NwcConnectionState::Reconnecting)));
        assert_eq!(
            backend
                .snapshot(WalletProjectionScope::default())
                .projection
                .readiness,
            WalletReadiness::Activating
        );
    }

    #[test]
    fn poisoned_status_mutex_is_recovered_not_collapsed_to_not_configured() {
        let (backend, status_slot) = backend();
        *status_slot.lock().unwrap() = Some(status("ready", None));

        // Poison the mutex the same way a panicking writer would.
        let poison_slot = status_slot.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poison_slot.lock().unwrap();
            panic!("simulated writer panic while holding the status lock");
        })
        .join();
        assert!(status_slot.is_poisoned());

        assert_eq!(
            backend
                .snapshot(WalletProjectionScope::default())
                .projection
                .readiness,
            WalletReadiness::Ready,
            "a poisoned lock must recover the last-written status, not report NotConfigured"
        );
    }

    #[test]
    fn on_wallet_event_and_on_mint_result_are_no_ops() {
        let (backend, _status) = backend();

        let event = KernelEvent {
            id: "d".repeat(64),
            author: "e".repeat(64),
            kind: 23_195,
            created_at: 0,
            tags: Vec::new(),
            content: "encrypted-payload".to_string(),
            relay_provenance: Vec::new(),
        };
        assert!(backend.on_wallet_event(ctx(), &event).is_empty());

        let result = MintResult {
            operation_id: "op-1".to_string(),
            status: crate::backend::MintResultStatus::Settled,
        };
        assert!(backend.on_mint_result(ctx(), result).is_empty());
    }
}
