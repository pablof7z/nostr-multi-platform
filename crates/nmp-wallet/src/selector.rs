//! W4 (#2908, epic #2864) — backend selection.
//!
//! [`WalletBackendSelector`] is the routing layer between a dispatched
//! [`WalletIntent`] and the concrete [`WalletBackend`] that should execute
//! it. Selection is **capability-driven**, not identity-driven: a caller
//! never names a backend directly to pay/create/deposit/etc. — it dispatches
//! an intent, and the selector picks whichever registered backend advertises
//! the capability that intent requires. The one identity-driven exception is
//! `nmp.wallet.select_backend`, which sets a *preference* consulted only when
//! more than one registered backend could satisfy the same capability — not
//! reachable today (NWC's and Cashu's capability sets are disjoint; see
//! `tests::nwc_and_cashu_capabilities_never_overlap_today`), but load-bearing
//! once e.g. Cashu melt implements `pay_bolt11` alongside NWC
//! (nip60-nip61-wallet-design.md, "Capability flags").
//!
//! Fail-closed everywhere: zero capable backends, or (today-unreachable)
//! ambiguity between more than one with no preference set, both produce a
//! structured `UiToken` error (see `crate::fail_closed::fail_closed`) rather
//! than a panic or a silent no-op.

use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;

use crate::backend::{
    WalletBackend, WalletBackendContext, WalletBackendId, WalletBackendSnapshot, WalletIntent,
    WalletProjectionScope,
};
use crate::capability::{WalletCapabilities, WalletCapability};
use crate::fail_closed::fail_closed;
use crate::projection::{WalletProjection, WalletReadiness};
use crate::ui_codes;

/// Fail-closed reasons [`WalletBackendSelector::resolve`] can reject a
/// capability lookup with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SelectorError {
    /// No registered backend advertises the required capability.
    NoCapableBackend,
    /// More than one registered backend advertises the required capability
    /// and no preferred backend (or a preferred backend not among the
    /// candidates) resolves the tie.
    AmbiguousSelection,
    /// `set_preferred` named a `backend_id` no registered backend carries.
    UnknownBackend(WalletBackendId),
}

/// Maps a [`WalletIntent`] to the [`WalletCapability`] a backend must
/// advertise to execute it. `SelectBackend` has no backend-capability
/// dimension — it is handled directly by the selector, never dispatched to a
/// backend — so it maps to `None`.
#[must_use]
pub fn capability_for(intent: &WalletIntent) -> Option<WalletCapability> {
    match intent {
        WalletIntent::SelectBackend { .. } => None,
        WalletIntent::PayBolt11 { .. } => Some(WalletCapability::PayBolt11),
        // NOTE: `CreateCashuWallet` and `RecoverCashuWallet` share ONE
        // capability flag (`WalletCapabilities::action_namespaces` bundles
        // both action namespaces under `create_cashu_wallet`), so this
        // mapping alone cannot distinguish "a backend can create" from "a
        // backend can also recover". `action::cashu::CashuRecoverModule`
        // rejects unconditionally in its own `start()` rather than trusting
        // capability resolution here — see `ui_codes::CASHU_RECOVER_NOT_IMPLEMENTED`.
        WalletIntent::CreateCashuWallet { .. } | WalletIntent::RecoverCashuWallet => {
            Some(WalletCapability::CreateCashuWallet)
        }
        WalletIntent::PublishNutzapInfo => Some(WalletCapability::PublishNutzapInfo),
        WalletIntent::SendNutzap { .. } => Some(WalletCapability::SendNutzap),
        WalletIntent::RedeemNutzap { .. } => Some(WalletCapability::RedeemNutzap),
        WalletIntent::DepositQuoteCashu { .. } | WalletIntent::CompleteDepositCashu { .. } => {
            Some(WalletCapability::DepositCashu)
        }
        WalletIntent::MeltCashu { .. } => Some(WalletCapability::MeltCashu),
    }
}

/// Routes [`WalletIntent`]s to the registered [`WalletBackend`] whose
/// capabilities satisfy them, and folds every registered backend's
/// [`WalletBackendSnapshot`] into one bounded [`WalletProjection`].
pub struct WalletBackendSelector {
    backends: Vec<Arc<dyn WalletBackend>>,
    preferred: Mutex<Option<WalletBackendId>>,
}

impl WalletBackendSelector {
    #[must_use]
    pub fn new(backends: Vec<Arc<dyn WalletBackend>>) -> Self {
        Self {
            backends,
            preferred: Mutex::new(None),
        }
    }

    /// Registered backend ids, in registration order.
    #[must_use]
    pub fn backend_ids(&self) -> Vec<WalletBackendId> {
        self.backends.iter().map(|b| b.id()).collect()
    }

    #[must_use]
    pub fn has_backend(&self, id: &WalletBackendId) -> bool {
        self.backends.iter().any(|b| &b.id() == id)
    }

    /// The currently preferred backend, if one has been set via
    /// `nmp.wallet.select_backend`. A poisoned lock is treated as "none set"
    /// (D6) — falling back to `resolve`'s single-candidate/fail-closed path
    /// is safer than propagating a lock-poison panic.
    #[must_use]
    pub fn preferred(&self) -> Option<WalletBackendId> {
        self.preferred.lock().ok().and_then(|guard| guard.clone())
    }

    /// Set the preferred backend. Fails closed — never silently ignored —
    /// when `id` does not name a registered backend.
    pub fn set_preferred(&self, id: WalletBackendId) -> Result<(), SelectorError> {
        if !self.has_backend(&id) {
            return Err(SelectorError::UnknownBackend(id));
        }
        if let Ok(mut guard) = self.preferred.lock() {
            *guard = Some(id);
        }
        Ok(())
    }

    /// Backends advertising `capability`, in registration order.
    #[must_use]
    pub fn candidates_for(&self, capability: WalletCapability) -> Vec<Arc<dyn WalletBackend>> {
        self.backends
            .iter()
            .filter(|b| b.capabilities().supports(capability))
            .cloned()
            .collect()
    }

    /// Resolve the single backend that should execute `capability`.
    ///
    /// - Zero candidates → `NoCapableBackend` (absent capability, fail closed).
    /// - One candidate → that backend, regardless of preference.
    /// - More than one candidate: the preferred backend if it is among them;
    ///   otherwise `AmbiguousSelection` (never guess which backend should
    ///   move money — money-safety over convenience).
    pub fn resolve(
        &self,
        capability: WalletCapability,
    ) -> Result<Arc<dyn WalletBackend>, SelectorError> {
        let candidates = self.candidates_for(capability);
        match candidates.len() {
            0 => Err(SelectorError::NoCapableBackend),
            1 => Ok(candidates.into_iter().next().expect("len checked")),
            _ => {
                let preferred = self.preferred();
                preferred
                    .and_then(|id| candidates.iter().find(|b| b.id() == id).cloned())
                    .ok_or(SelectorError::AmbiguousSelection)
            }
        }
    }

    /// Resolve `intent`'s backend and dispatch it, or fail closed.
    ///
    /// Never called with `WalletIntent::SelectBackend` — that variant is
    /// handled directly by [`Self::set_preferred`]
    /// (`action::select_backend::SelectBackendModule`), not routed through a
    /// backend.
    #[must_use]
    pub fn dispatch(
        &self,
        ctx: WalletBackendContext<'_>,
        intent: WalletIntent,
        correlation_id: Option<String>,
    ) -> Vec<ActorCommand> {
        let Some(capability) = capability_for(&intent) else {
            // Unreachable via the registered action modules (SelectBackend
            // never reaches `dispatch`); fail closed rather than panic if a
            // future caller does.
            return fail_closed(
                ui_codes::NO_CAPABLE_BACKEND,
                correlation_id,
                "select_backend has no backend-routed capability".to_string(),
            );
        };
        match self.resolve(capability) {
            Ok(backend) => backend.start_intent(ctx, intent, correlation_id),
            Err(SelectorError::NoCapableBackend) => fail_closed(
                ui_codes::NO_CAPABLE_BACKEND,
                correlation_id,
                "no registered wallet backend supports this operation".to_string(),
            ),
            Err(SelectorError::AmbiguousSelection) => fail_closed(
                ui_codes::AMBIGUOUS_BACKEND_SELECTION,
                correlation_id,
                "multiple wallet backends could handle this operation; select one first"
                    .to_string(),
            ),
            Err(SelectorError::UnknownBackend(_)) => fail_closed(
                ui_codes::UNKNOWN_BACKEND,
                correlation_id,
                "selected backend is not registered".to_string(),
            ),
        }
    }

    /// Union of every registered backend's advertised capabilities — the
    /// projection surfaces every action namespace ANY backend can execute,
    /// not just the currently-preferred one.
    #[must_use]
    pub fn union_capabilities(&self) -> WalletCapabilities {
        self.backends
            .iter()
            .map(|b| b.capabilities())
            .fold(WalletCapabilities::none(), union_capabilities)
    }

    /// Fold every registered backend's [`WalletBackendSnapshot`] into one
    /// bounded [`WalletProjection`].
    ///
    /// Merge policy: every backend's own `snapshot()` unconditionally
    /// reports `active_backend_id: Some(self.id())` regardless of
    /// connection/readiness (see `NwcWalletBackend`/`CashuWalletBackend`'s
    /// impls) — that field identifies WHO produced a snapshot, not whether
    /// it is the merged view's "active" one, so this merge does not read it.
    /// Instead: with a preference set (and registered), `active_backend_id`/
    /// `readiness` are that backend's own, respecting the explicit choice
    /// even if another backend happens to be more "ready". Without one,
    /// `readiness` is the best (most-ready) readiness among ALL registered
    /// backends (`readiness_rank`) — so with today's two backends, the
    /// merged view shows `Ready` the moment EITHER is ready, rather than
    /// getting stuck at `NotConfigured` forever pending an explicit
    /// `select_backend` call nothing requires a user to ever make.
    /// `capabilities` is the union from `union_capabilities`. Balances/
    /// pending operations/history/receive rows concatenate across every
    /// backend, bounded by the existing `MAX_WALLET_PROJECTION_ROWS`
    /// machinery in `projection.rs`. `cashu_p2pk_pubkey`/
    /// `accepted_mint_count`/`accepted_relay_count` are Cashu-shaped fields
    /// with no NWC analogue — take the first non-default value/sum across
    /// backends respectively.
    #[must_use]
    pub fn snapshot(&self, scope: WalletProjectionScope) -> WalletProjection {
        let snapshots: Vec<WalletBackendSnapshot> =
            self.backends.iter().map(|b| b.snapshot(scope)).collect();

        let (active_id, readiness) = self.merged_active_backend(&snapshots);

        let mut projection = WalletProjection::new(active_id, readiness, self.union_capabilities());

        projection = projection
            .with_balances(snapshots.iter().flat_map(|s| s.projection.balances.clone()))
            .with_pending_operations(
                snapshots
                    .iter()
                    .flat_map(|s| s.projection.pending_operations.clone()),
            )
            .with_recent_history(
                snapshots
                    .iter()
                    .flat_map(|s| s.projection.recent_history.clone()),
            )
            .with_receive_rows(
                snapshots
                    .iter()
                    .flat_map(|s| s.projection.receive_rows.clone()),
            );

        projection.cashu_p2pk_pubkey = snapshots
            .iter()
            .find_map(|s| s.projection.cashu_p2pk_pubkey.clone());
        projection.accepted_mint_count = snapshots
            .iter()
            .map(|s| s.projection.accepted_mint_count)
            .sum();
        projection.accepted_relay_count = snapshots
            .iter()
            .map(|s| s.projection.accepted_relay_count)
            .sum();

        projection
    }

    /// The "active" backend id + readiness for merged-projection purposes.
    /// See [`Self::snapshot`]'s doc comment for the merge policy. `snapshots`
    /// must be `self.backends.iter().map(|b| b.snapshot(..))` in the same
    /// order (zipped by index, not by any self-reported id) — the only
    /// caller, `snapshot`, upholds this.
    fn merged_active_backend(
        &self,
        snapshots: &[WalletBackendSnapshot],
    ) -> (Option<WalletBackendId>, WalletReadiness) {
        if let Some(preferred) = self.preferred() {
            if let Some((backend, snap)) = self
                .backends
                .iter()
                .zip(snapshots)
                .find(|(b, _)| b.id() == preferred)
            {
                return (Some(backend.id()), snap.projection.readiness);
            }
        }
        self.backends
            .iter()
            .zip(snapshots)
            .max_by_key(|(_, snap)| readiness_rank(snap.projection.readiness))
            .map(|(backend, snap)| (Some(backend.id()), snap.projection.readiness))
            .unwrap_or((None, WalletReadiness::NotConfigured))
    }

    /// Registered backends, exposed for the runtime/observer to route
    /// `on_wallet_event`/`on_mint_result` without re-deriving capability
    /// resolution (those callbacks are addressed by backend id, not by
    /// capability — see `runtime::WalletRuntime`).
    #[must_use]
    pub fn backend_by_id(&self, id: &WalletBackendId) -> Option<Arc<dyn WalletBackend>> {
        self.backends.iter().find(|b| &b.id() == id).cloned()
    }

    /// All registered backends — the observer broadcasts an observed
    /// `KernelEvent` to every backend's `on_wallet_event` (each backend
    /// decides for itself whether the event is relevant; both backends
    /// already no-op events outside their own concern, see their doc
    /// comments), since a durable wallet event is not addressed to a single
    /// backend the way a dispatched intent is.
    #[must_use]
    pub fn backends(&self) -> &[Arc<dyn WalletBackend>] {
        &self.backends
    }
}

/// Ranks readiness from least to most "the wallet is doing something useful"
/// for [`WalletBackendSelector::merged_active_backend`]'s best-of-N merge.
/// `Degraded` outranks `Activating`: a backend that WAS ready and is now
/// degraded still has more state/history behind it than one still coming up.
fn readiness_rank(readiness: WalletReadiness) -> u8 {
    match readiness {
        WalletReadiness::NotConfigured => 0,
        WalletReadiness::Activating => 1,
        WalletReadiness::Degraded => 2,
        WalletReadiness::Ready => 3,
    }
}

fn union_capabilities(acc: WalletCapabilities, next: WalletCapabilities) -> WalletCapabilities {
    WalletCapabilities {
        pay_bolt11: acc.pay_bolt11 || next.pay_bolt11,
        create_cashu_wallet: acc.create_cashu_wallet || next.create_cashu_wallet,
        publish_nutzap_info: acc.publish_nutzap_info || next.publish_nutzap_info,
        send_nutzap: acc.send_nutzap || next.send_nutzap,
        redeem_nutzap: acc.redeem_nutzap || next.redeem_nutzap,
        deposit_cashu: acc.deposit_cashu || next.deposit_cashu,
        melt_cashu: acc.melt_cashu || next.melt_cashu,
        observe_nutzap_receipts: acc.observe_nutzap_receipts || next.observe_nutzap_receipts,
    }
}

#[cfg(test)]
#[path = "selector_tests.rs"]
mod tests;
