use std::sync::Mutex;

use super::*;
use crate::ui_codes;
use crate::WalletBackendId;
use nmp_core::substrate::ActionContext;

fn ctx() -> ActionContext {
    ActionContext::default()
}

fn active_pubkey(pubkey: &str) -> ActiveAccountSlot {
    Arc::new(Mutex::new(Some(pubkey.to_string())))
}

const PK: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// Today's real backends (NWC, Cashu) never advertise the nutzap
/// capabilities — the Cashu backend's `cashu_wallet_and_deposit()` doc
/// comment explicitly says it does not implement nutzap send/receive yet —
/// so every nutzap module must fail closed at `start()` against the crate's
/// REAL selector composition, not just an empty stub selector.
fn real_backend_selector() -> Arc<WalletBackendSelector> {
    Arc::new(WalletBackendSelector::new(vec![Arc::new(
        crate::backend::cashu::CashuWalletBackend::new(),
    )]))
}

#[test]
fn publish_info_fails_closed_against_todays_real_backends() {
    let module = NutzapPublishInfoModule::new(real_backend_selector(), active_pubkey(PK));
    let err = module
        .start(&mut ctx(), NutzapPublishInfoAction {})
        .expect_err("no backend advertises publish_nutzap_info today");
    match err {
        ActionRejection::InvalidCoded { code, .. } => {
            assert_eq!(code, ui_codes::NO_CAPABLE_BACKEND)
        }
        other => panic!("expected InvalidCoded, got {other:?}"),
    }
}

#[test]
fn send_start_rejects_empty_recipient() {
    let module = NutzapSendModule::new(real_backend_selector(), active_pubkey(PK));
    let err = module
        .start(
            &mut ctx(),
            NutzapSendAction {
                recipient_pubkey: String::new(),
                amount_sats: 21,
                target_event_id: None,
            },
        )
        .expect_err("empty recipient_pubkey must be rejected");
    assert!(matches!(err, ActionRejection::Invalid(_)));
}

#[test]
fn send_start_rejects_zero_amount() {
    let module = NutzapSendModule::new(real_backend_selector(), active_pubkey(PK));
    let err = module
        .start(
            &mut ctx(),
            NutzapSendAction {
                recipient_pubkey: PK.to_string(),
                amount_sats: 0,
                target_event_id: None,
            },
        )
        .expect_err("zero amount_sats must be rejected");
    assert!(matches!(err, ActionRejection::Invalid(_)));
}

#[test]
fn send_fails_closed_against_todays_real_backends() {
    let module = NutzapSendModule::new(real_backend_selector(), active_pubkey(PK));
    let err = module
        .start(
            &mut ctx(),
            NutzapSendAction {
                recipient_pubkey: PK.to_string(),
                amount_sats: 21,
                target_event_id: None,
            },
        )
        .expect_err("no backend advertises send_nutzap today");
    match err {
        ActionRejection::InvalidCoded { code, .. } => {
            assert_eq!(code, ui_codes::NO_CAPABLE_BACKEND)
        }
        other => panic!("expected InvalidCoded, got {other:?}"),
    }
}

#[test]
fn redeem_start_rejects_empty_event_id() {
    let module = NutzapRedeemModule::new(real_backend_selector(), active_pubkey(PK));
    let err = module
        .start(
            &mut ctx(),
            NutzapRedeemAction {
                event_id: String::new(),
            },
        )
        .expect_err("empty event_id must be rejected");
    assert!(matches!(err, ActionRejection::Invalid(_)));
}

#[test]
fn redeem_fails_closed_against_todays_real_backends() {
    let module = NutzapRedeemModule::new(real_backend_selector(), active_pubkey(PK));
    let err = module
        .start(
            &mut ctx(),
            NutzapRedeemAction {
                event_id: "e".repeat(64),
            },
        )
        .expect_err("no backend advertises redeem_nutzap today");
    match err {
        ActionRejection::InvalidCoded { code, .. } => {
            assert_eq!(code, ui_codes::NO_CAPABLE_BACKEND)
        }
        other => panic!("expected InvalidCoded, got {other:?}"),
    }
}

/// Proves the dispatch point is real plumbing, not dead code: with a stub
/// backend that DOES advertise `send_nutzap`, `execute()` must reach it.
#[test]
fn send_execute_reaches_a_backend_that_advertises_the_capability() {
    struct StubSendBackend;
    impl crate::backend::WalletBackend for StubSendBackend {
        fn id(&self) -> WalletBackendId {
            WalletBackendId::new("stub-nutzap")
        }
        fn capabilities(&self) -> crate::capability::WalletCapabilities {
            crate::capability::WalletCapabilities {
                send_nutzap: true,
                ..crate::capability::WalletCapabilities::none()
            }
        }
        fn snapshot(
            &self,
            _scope: crate::backend::WalletProjectionScope,
        ) -> crate::backend::WalletBackendSnapshot {
            crate::backend::WalletBackendSnapshot {
                projection: crate::projection::WalletProjection::new(
                    Some(self.id()),
                    crate::projection::WalletReadiness::Ready,
                    self.capabilities(),
                ),
            }
        }
        fn start_intent(
            &self,
            _ctx: crate::backend::WalletBackendContext<'_>,
            _intent: WalletIntent,
            _correlation_id: Option<String>,
        ) -> Vec<ActorCommand> {
            vec![ActorCommand::ShowToast {
                message: "nutzap-sent".to_string(),
            }]
        }
        fn on_wallet_event(
            &self,
            _ctx: crate::backend::WalletBackendContext<'_>,
            _event: &nmp_core::substrate::KernelEvent,
        ) -> Vec<ActorCommand> {
            Vec::new()
        }
        fn on_mint_result(
            &self,
            _ctx: crate::backend::WalletBackendContext<'_>,
            _result: crate::backend::MintResult,
        ) -> Vec<ActorCommand> {
            Vec::new()
        }
    }

    let selector = Arc::new(WalletBackendSelector::new(vec![Arc::new(StubSendBackend)]));
    let module = NutzapSendModule::new(selector, active_pubkey(PK));
    let dispatched = std::cell::Cell::new(0);
    module
        .execute(
            &ctx(),
            NutzapSendAction {
                recipient_pubkey: PK.to_string(),
                amount_sats: 21,
                target_event_id: None,
            },
            "corr-1",
            &|_cmd| dispatched.set(dispatched.get() + 1),
        )
        .expect("execute must succeed");
    assert_eq!(dispatched.get(), 1);
}
