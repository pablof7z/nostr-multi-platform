use super::*;
use crate::backend::cashu::CashuWalletBackend;
use crate::backend::MintResult;
use nmp_core::substrate::KernelEvent;

/// A stub backend with a caller-controlled capability set, for testing
/// selection without a real NWC/Cashu adapter.
struct StubBackend {
    id: &'static str,
    caps: WalletCapabilities,
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
            projection: WalletProjection::new(
                Some(self.id()),
                crate::projection::WalletReadiness::Ready,
                self.caps,
            ),
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
        Arc::new(StubBackend {
            id: "a",
            caps: WalletCapabilities::nwc_payments(),
        }),
        Arc::new(StubBackend {
            id: "b",
            caps: WalletCapabilities::nwc_payments(),
        }),
    ]);
    assert!(matches!(
        selector.resolve(WalletCapability::PayBolt11),
        Err(SelectorError::AmbiguousSelection)
    ));
}

#[test]
fn preference_breaks_a_tie_between_two_capable_backends() {
    let selector = WalletBackendSelector::new(vec![
        Arc::new(StubBackend {
            id: "a",
            caps: WalletCapabilities::nwc_payments(),
        }),
        Arc::new(StubBackend {
            id: "b",
            caps: WalletCapabilities::nwc_payments(),
        }),
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
    let selector = WalletBackendSelector::new(vec![Arc::new(StubBackend {
        id: "a",
        caps: WalletCapabilities::nwc_payments(),
    })]);
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
        Arc::new(StubBackend {
            id: "nwc",
            caps: WalletCapabilities::nwc_payments(),
        }),
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

#[test]
fn snapshot_has_no_active_backend_when_ambiguous_and_unpreferred() {
    let selector = WalletBackendSelector::new(vec![
        Arc::new(StubBackend {
            id: "a",
            caps: WalletCapabilities::nwc_payments(),
        }),
        Arc::new(StubBackend {
            id: "b",
            caps: WalletCapabilities::cashu_wallet_and_deposit(),
        }),
    ]);
    let projection = selector.snapshot(WalletProjectionScope::default());
    assert_eq!(projection.active_backend_id, None);
    assert_eq!(
        projection.readiness,
        crate::projection::WalletReadiness::NotConfigured
    );
}
