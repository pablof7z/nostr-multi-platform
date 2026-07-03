use super::*;
use crate::backend::cashu::CashuWalletBackend;
use nmp_core::substrate::ActionContext;

fn ctx() -> ActionContext {
    ActionContext::default()
}

fn selector_with_cashu() -> Arc<WalletBackendSelector> {
    Arc::new(WalletBackendSelector::new(vec![Arc::new(
        CashuWalletBackend::new(),
    )]))
}

#[test]
fn start_rejects_empty_backend_id() {
    let module = SelectBackendModule::new(selector_with_cashu());
    let err = module
        .start(
            &mut ctx(),
            SelectBackendAction {
                backend_id: String::new(),
            },
        )
        .expect_err("empty backend_id must be rejected");
    assert!(matches!(err, ActionRejection::Invalid(_)));
}

#[test]
fn start_rejects_an_unregistered_backend_id() {
    let module = SelectBackendModule::new(selector_with_cashu());
    let err = module
        .start(
            &mut ctx(),
            SelectBackendAction {
                backend_id: "does-not-exist".to_string(),
            },
        )
        .expect_err("unregistered backend_id must be rejected");
    match err {
        ActionRejection::InvalidCoded { code, .. } => {
            assert_eq!(code, ui_codes::UNKNOWN_BACKEND);
        }
        other => panic!("expected InvalidCoded, got {other:?}"),
    }
}

#[test]
fn start_accepts_a_registered_backend_id() {
    let module = SelectBackendModule::new(selector_with_cashu());
    module
        .start(
            &mut ctx(),
            SelectBackendAction {
                backend_id: crate::CASHU_BACKEND_ID.to_string(),
            },
        )
        .expect("registered backend_id must be accepted");
}

#[test]
fn execute_sets_the_selectors_preferred_backend() {
    let selector = selector_with_cashu();
    let module = SelectBackendModule::new(Arc::clone(&selector));
    module
        .execute(
            &ctx(),
            SelectBackendAction {
                backend_id: crate::CASHU_BACKEND_ID.to_string(),
            },
            "corr-1",
            &|_cmd| panic!("select_backend must not dispatch any ActorCommand"),
        )
        .expect("execute must succeed for a registered backend");
    assert_eq!(
        selector.preferred().as_ref().map(|id| id.as_str()),
        Some(crate::CASHU_BACKEND_ID)
    );
}
