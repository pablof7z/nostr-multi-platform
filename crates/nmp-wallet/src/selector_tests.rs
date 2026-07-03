use super::*;
use crate::backend::cashu::CashuWalletBackend;
use crate::backend::MintResult;
use nmp_core::actor::ActionLedgerCommand;
use nmp_core::substrate::KernelEvent;

/// A stub backend with a caller-controlled capability set and readiness, for
/// testing selection/merge without a real NWC/Cashu adapter.
struct StubBackend {
    id: &'static str,
    caps: WalletCapabilities,
    readiness: crate::projection::WalletReadiness,
}

impl StubBackend {
    fn new(id: &'static str, caps: WalletCapabilities) -> Self {
        Self {
            id,
            caps,
            readiness: crate::projection::WalletReadiness::Ready,
        }
    }

    fn with_readiness(mut self, readiness: crate::projection::WalletReadiness) -> Self {
        self.readiness = readiness;
        self
    }
}

impl WalletBackend for StubBackend {
    fn id(&self) -> WalletBackendId {
        WalletBackendId::new(self.id)
    }

    fn capabilities(&self) -> WalletCapabilities {
        self.caps
    }

    fn snapshot(&self, _scope: WalletProjectionScope) -> WalletBackendSnapshot {
        WalletBackendSnapshot {
            projection: WalletProjection::new(Some(self.id()), self.readiness, self.caps),
        }
    }

    fn start_intent(
        &self,
        _ctx: WalletBackendContext<'_>,
        _intent: WalletIntent,
        _correlation_id: Option<String>,
    ) -> Vec<ActorCommand> {
        vec![ActorCommand::ShowToast {
            message: format!("{}-dispatched", self.id),
        }]
    }

    fn on_wallet_event(
        &self,
        _ctx: WalletBackendContext<'_>,
        _event: &KernelEvent,
    ) -> Vec<ActorCommand> {
        Vec::new()
    }

    fn on_mint_result(
        &self,
        _ctx: WalletBackendContext<'_>,
        _result: MintResult,
    ) -> Vec<ActorCommand> {
        Vec::new()
    }
}

fn ctx() -> WalletBackendContext<'static> {
    WalletBackendContext {
        now_secs: 0,
        selected_backend: None,
        account_pubkey: None,
    }
}

/// The claim `dispatch`'s doc comment and `WalletCapabilities::action_namespaces`
/// rely on: NWC's and Cashu's real capability sets do not overlap today,
/// so `AmbiguousSelection` is unreachable in production — only exercised
/// here via stub backends.
#[test]
fn nwc_and_cashu_capabilities_never_overlap_today() {
    let nwc = WalletCapabilities::nwc_payments();
    let cashu = WalletCapabilities::cashu_wallet_and_deposit();
    assert_eq!(
        union_capabilities(nwc, cashu),
        WalletCapabilities {
            pay_bolt11: true,
            create_cashu_wallet: true,
            deposit_cashu: true,
            ..WalletCapabilities::none()
        }
    );
    // No capability is true in both.
    assert!(!(nwc.pay_bolt11 && cashu.pay_bolt11));
    assert!(!(nwc.create_cashu_wallet && cashu.create_cashu_wallet));
    assert!(!(nwc.deposit_cashu && cashu.deposit_cashu));
}

#[test]
fn resolve_fails_closed_with_zero_backends() {
    let selector = WalletBackendSelector::new(Vec::new());
    assert!(matches!(
        selector.resolve(WalletCapability::PayBolt11),
        Err(SelectorError::NoCapableBackend)
    ));
}

#[test]
fn resolve_picks_the_sole_capable_backend_without_a_preference() {
    let selector = WalletBackendSelector::new(vec![Arc::new(CashuWalletBackend::new())]);
    let backend = selector
        .resolve(WalletCapability::CreateCashuWallet)
        .expect("cashu backend supports create_cashu_wallet");
    assert_eq!(backend.id().as_str(), crate::CASHU_BACKEND_ID);
}

#[test]
fn resolve_is_ambiguous_with_two_capable_backends_and_no_preference() {
    let selector = WalletBackendSelector::new(vec![
        Arc::new(StubBackend::new("a", WalletCapabilities::nwc_payments())),
        Arc::new(StubBackend::new("b", WalletCapabilities::nwc_payments())),
    ]);
    assert!(matches!(
        selector.resolve(WalletCapability::PayBolt11),
        Err(SelectorError::AmbiguousSelection)
    ));
}

#[test]
fn preference_breaks_a_tie_between_two_capable_backends() {
    let selector = WalletBackendSelector::new(vec![
        Arc::new(StubBackend::new("a", WalletCapabilities::nwc_payments())),
        Arc::new(StubBackend::new("b", WalletCapabilities::nwc_payments())),
    ]);
    selector
        .set_preferred(WalletBackendId::new("b"))
        .expect("b is registered");
    let backend = selector
        .resolve(WalletCapability::PayBolt11)
        .expect("preference resolves the tie");
    assert_eq!(backend.id().as_str(), "b");
}

#[test]
fn set_preferred_fails_closed_for_an_unregistered_backend_id() {
    let selector = WalletBackendSelector::new(vec![Arc::new(CashuWalletBackend::new())]);
    assert_eq!(
        selector.set_preferred(WalletBackendId::new("nope")),
        Err(SelectorError::UnknownBackend(WalletBackendId::new("nope")))
    );
    assert_eq!(selector.preferred(), None);
}

#[test]
fn dispatch_routes_to_the_capable_backend() {
    let selector = WalletBackendSelector::new(vec![Arc::new(StubBackend::new(
        "a",
        WalletCapabilities::nwc_payments(),
    ))]);
    let commands = selector.dispatch(
        ctx(),
        WalletIntent::PayBolt11 {
            bolt11: "lnbc1".to_string(),
            amount_msats: None,
        },
        Some("corr-1".to_string()),
    );
    assert_eq!(commands.len(), 1);
    assert!(matches!(commands[0], ActorCommand::ShowToast { .. }));
}

#[test]
fn dispatch_fails_closed_with_an_error_token_and_ledger_failure_when_no_backend_is_capable() {
    let selector = WalletBackendSelector::new(Vec::new());
    let commands = selector.dispatch(
        ctx(),
        WalletIntent::PayBolt11 {
            bolt11: "lnbc1".to_string(),
            amount_msats: None,
        },
        Some("corr-1".to_string()),
    );
    assert_eq!(commands.len(), 2);
    assert!(matches!(commands[0], ActorCommand::ShowErrorToken { .. }));
    assert!(matches!(
        commands[1],
        ActorCommand::ActionLedger(ActionLedgerCommand::RecordFailure { .. })
    ));
}

#[test]
fn union_capabilities_surfaces_every_backend_action_namespace() {
    let selector = WalletBackendSelector::new(vec![
        Arc::new(StubBackend::new("nwc", WalletCapabilities::nwc_payments())),
        Arc::new(CashuWalletBackend::new()),
    ]);
    let union = selector.union_capabilities();
    assert!(union.pay_bolt11);
    assert!(union.create_cashu_wallet);
    assert!(union.deposit_cashu);
}

#[test]
fn snapshot_merges_active_backend_from_the_sole_registered_backend() {
    let selector = WalletBackendSelector::new(vec![Arc::new(CashuWalletBackend::new())]);
    let projection = selector.snapshot(WalletProjectionScope::default());
    assert_eq!(
        projection.active_backend_id.as_ref().map(|id| id.as_str()),
        Some(crate::CASHU_BACKEND_ID)
    );
}

/// With no preference set, the merged view shows whichever backend is
/// currently the MOST ready, not `NotConfigured` forever — see
/// `WalletBackendSelector::snapshot`'s doc comment on why a fixed
/// "ambiguous means None" merge (the pre-fix behavior) was wrong: nothing
/// requires a user to ever call `select_backend`, so a merge that only
/// reports readiness once a preference is set would leave the top-level
/// wallet indicator permanently `NotConfigured` even once a backend is
/// genuinely ready.
#[test]
fn snapshot_reports_the_most_ready_backend_when_unpreferred() {
    let selector = WalletBackendSelector::new(vec![
        Arc::new(
            StubBackend::new("a", WalletCapabilities::nwc_payments())
                .with_readiness(crate::projection::WalletReadiness::NotConfigured),
        ),
        Arc::new(
            StubBackend::new("b", WalletCapabilities::cashu_wallet_and_deposit())
                .with_readiness(crate::projection::WalletReadiness::Ready),
        ),
    ]);
    let projection = selector.snapshot(WalletProjectionScope::default());
    assert_eq!(
        projection.active_backend_id.as_ref().map(|id| id.as_str()),
        Some("b")
    );
    assert_eq!(
        projection.readiness,
        crate::projection::WalletReadiness::Ready
    );
}

/// A preference wins even when it names a LESS-ready backend than another
/// registered one — the user's explicit choice is authoritative, not just a
/// tie-breaker for equally-ready candidates.
#[test]
fn snapshot_respects_an_explicit_preference_over_a_more_ready_backend() {
    let selector = WalletBackendSelector::new(vec![
        Arc::new(
            StubBackend::new("a", WalletCapabilities::nwc_payments())
                .with_readiness(crate::projection::WalletReadiness::Activating),
        ),
        Arc::new(
            StubBackend::new("b", WalletCapabilities::cashu_wallet_and_deposit())
                .with_readiness(crate::projection::WalletReadiness::Ready),
        ),
    ]);
    selector
        .set_preferred(WalletBackendId::new("a"))
        .expect("a is registered");
    let projection = selector.snapshot(WalletProjectionScope::default());
    assert_eq!(
        projection.active_backend_id.as_ref().map(|id| id.as_str()),
        Some("a")
    );
    assert_eq!(
        projection.readiness,
        crate::projection::WalletReadiness::Activating
    );
}

/// Every registered backend unconditionally reports itself as
/// `active_backend_id` from its OWN `snapshot()` (see
/// `NwcWalletBackend`/`CashuWalletBackend`'s impls) — the merge must not be
/// fooled by that self-report into picking a stale/wrong backend; it must
/// derive the merged `active_backend_id` from which backend actually won
/// the readiness comparison, zipped by registration order.
#[test]
fn snapshot_merge_ignores_each_backends_self_reported_active_id_and_uses_actual_readiness() {
    let selector = WalletBackendSelector::new(vec![
        Arc::new(
            StubBackend::new("a", WalletCapabilities::nwc_payments())
                .with_readiness(crate::projection::WalletReadiness::Degraded),
        ),
        Arc::new(
            StubBackend::new("b", WalletCapabilities::cashu_wallet_and_deposit())
                .with_readiness(crate::projection::WalletReadiness::Activating),
        ),
    ]);
    let projection = selector.snapshot(WalletProjectionScope::default());
    // "a" is Degraded (rank 2) and "b" is Activating (rank 1) — "a" must win
    // even though both backends' own snapshots equally claim
    // `active_backend_id: Some(self.id())`.
    assert_eq!(
        projection.active_backend_id.as_ref().map(|id| id.as_str()),
        Some("a")
    );
    assert_eq!(
        projection.readiness,
        crate::projection::WalletReadiness::Degraded
    );
}
