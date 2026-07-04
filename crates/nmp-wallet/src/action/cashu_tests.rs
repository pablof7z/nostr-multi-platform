use std::sync::Mutex;

use super::*;
use nmp_core::substrate::ActionContext;

fn ctx() -> ActionContext {
    ActionContext::default()
}

fn empty_selector() -> Arc<WalletBackendSelector> {
    Arc::new(WalletBackendSelector::new(Vec::new()))
}

fn cashu_selector() -> Arc<WalletBackendSelector> {
    Arc::new(WalletBackendSelector::new(vec![Arc::new(
        crate::backend::cashu::CashuWalletBackend::new(),
    )]))
}

fn active_pubkey(pubkey: Option<&str>) -> ActiveAccountSlot {
    std::sync::Arc::new(Mutex::new(pubkey.map(str::to_string)))
}

const MINT: &str = "https://mint.example";
const PK: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

// ── cashu.create ─────────────────────────────────────────────────────────────

#[test]
fn create_start_rejects_empty_mint() {
    let module = CashuCreateModule::new(cashu_selector(), active_pubkey(Some(PK)));
    let err = module
        .start(
            &mut ctx(),
            CashuCreateAction {
                mint: String::new(),
            },
        )
        .expect_err("empty mint must be rejected");
    assert!(matches!(err, ActionRejection::Invalid(_)));
}

#[test]
fn create_start_fails_closed_with_no_capable_backend() {
    let module = CashuCreateModule::new(empty_selector(), active_pubkey(Some(PK)));
    let err = module
        .start(
            &mut ctx(),
            CashuCreateAction {
                mint: MINT.to_string(),
            },
        )
        .expect_err("no registered backend must be rejected");
    match err {
        ActionRejection::InvalidCoded { code, .. } => {
            assert_eq!(code, ui_codes::NO_CAPABLE_BACKEND)
        }
        other => panic!("expected InvalidCoded, got {other:?}"),
    }
}

#[test]
fn create_execute_reaches_the_cashu_backend_and_dispatches_a_command() {
    let module = CashuCreateModule::new(cashu_selector(), active_pubkey(Some(PK)));
    let dispatched = std::cell::Cell::new(0);
    module
        .execute(
            &ctx(),
            CashuCreateAction {
                mint: MINT.to_string(),
            },
            "corr-1",
            &|_cmd| dispatched.set(dispatched.get() + 1),
        )
        .expect("execute must succeed");
    assert!(
        dispatched.get() > 0,
        "create must dispatch at least one command"
    );
}

// ── cashu.recover ────────────────────────────────────────────────────────────

#[test]
fn recover_start_fails_closed_with_no_capable_backend() {
    let module = CashuRecoverModule::new(empty_selector(), active_pubkey(Some(PK)));
    let err = module
        .start(&mut ctx(), CashuRecoverAction {})
        .expect_err("no registered backend must be rejected");
    match err {
        ActionRejection::InvalidCoded { code, .. } => {
            assert_eq!(code, ui_codes::NO_CAPABLE_BACKEND)
        }
        other => panic!("expected InvalidCoded, got {other:?}"),
    }
}

#[test]
fn recover_execute_reaches_the_cashu_backend_and_dispatches_a_command() {
    let module = CashuRecoverModule::new(cashu_selector(), active_pubkey(Some(PK)));
    let dispatched = std::cell::Cell::new(0);
    module
        .execute(&ctx(), CashuRecoverAction {}, "corr-1", &|_cmd| {
            dispatched.set(dispatched.get() + 1)
        })
        .expect("execute must succeed");
    assert!(
        dispatched.get() > 0,
        "recover must dispatch at least one command"
    );
}

// ── cashu.set_mints ──────────────────────────────────────────────────────────

#[test]
fn set_mints_start_rejects_empty_mint_list() {
    let module = CashuSetMintsModule::new(cashu_selector(), active_pubkey(Some(PK)));
    let err = module
        .start(&mut ctx(), CashuSetMintsAction { mints: Vec::new() })
        .expect_err("empty mint list must be rejected");
    assert!(matches!(err, ActionRejection::Invalid(_)));
}

#[test]
fn set_mints_start_rejects_a_malformed_mint_url() {
    let module = CashuSetMintsModule::new(cashu_selector(), active_pubkey(Some(PK)));
    let err = module
        .start(
            &mut ctx(),
            CashuSetMintsAction {
                mints: vec![MINT.to_string(), "not-a-url".to_string()],
            },
        )
        .expect_err("a malformed mint URL must be rejected");
    assert!(matches!(err, ActionRejection::Invalid(_)));
}

#[test]
fn set_mints_start_fails_closed_with_no_capable_backend() {
    let module = CashuSetMintsModule::new(empty_selector(), active_pubkey(Some(PK)));
    let err = module
        .start(
            &mut ctx(),
            CashuSetMintsAction {
                mints: vec![MINT.to_string()],
            },
        )
        .expect_err("no registered backend must be rejected");
    match err {
        ActionRejection::InvalidCoded { code, .. } => {
            assert_eq!(code, ui_codes::NO_CAPABLE_BACKEND)
        }
        other => panic!("expected InvalidCoded, got {other:?}"),
    }
}

#[test]
fn set_mints_execute_reaches_the_cashu_backend_and_dispatches_a_command() {
    // The backend fails closed (no wallet created yet), but `execute()` must
    // still reach it and emit at least one command — proving the dispatch
    // path, not the backend's own precondition, is what this test covers.
    let module = CashuSetMintsModule::new(cashu_selector(), active_pubkey(Some(PK)));
    let dispatched = std::cell::Cell::new(0);
    module
        .execute(
            &ctx(),
            CashuSetMintsAction {
                mints: vec![MINT.to_string()],
            },
            "corr-1",
            &|_cmd| dispatched.set(dispatched.get() + 1),
        )
        .expect("execute must succeed");
    assert!(
        dispatched.get() > 0,
        "set_mints must dispatch at least one command"
    );
}

// ── cashu.deposit_quote ──────────────────────────────────────────────────────

#[test]
fn deposit_quote_start_rejects_zero_amount() {
    let module = CashuDepositQuoteModule::new(cashu_selector(), active_pubkey(Some(PK)));
    let err = module
        .start(
            &mut ctx(),
            CashuDepositQuoteAction {
                mint: MINT.to_string(),
                amount_sats: 0,
            },
        )
        .expect_err("zero amount_sats must be rejected");
    assert!(matches!(err, ActionRejection::Invalid(_)));
}

#[test]
fn deposit_quote_start_fails_closed_with_no_capable_backend() {
    let module = CashuDepositQuoteModule::new(empty_selector(), active_pubkey(Some(PK)));
    let err = module
        .start(
            &mut ctx(),
            CashuDepositQuoteAction {
                mint: MINT.to_string(),
                amount_sats: 1_000,
            },
        )
        .expect_err("no registered backend must be rejected");
    match err {
        ActionRejection::InvalidCoded { code, .. } => {
            assert_eq!(code, ui_codes::NO_CAPABLE_BACKEND)
        }
        other => panic!("expected InvalidCoded, got {other:?}"),
    }
}

// ── cashu.complete_deposit ───────────────────────────────────────────────────

#[test]
fn complete_deposit_start_rejects_empty_quote_id() {
    let module = CashuCompleteDepositModule::new(cashu_selector(), active_pubkey(Some(PK)));
    let err = module
        .start(
            &mut ctx(),
            CashuCompleteDepositAction {
                quote_id: String::new(),
            },
        )
        .expect_err("empty quote_id must be rejected");
    assert!(matches!(err, ActionRejection::Invalid(_)));
}

#[test]
fn complete_deposit_execute_fails_closed_for_an_unknown_quote() {
    // No pending deposit has been started, so the backend's own fail-closed
    // path (`ui_codes::UNKNOWN_QUOTE`) fires — proving `execute()` really
    // reaches the backend rather than short-circuiting.
    let module = CashuCompleteDepositModule::new(cashu_selector(), active_pubkey(Some(PK)));
    let dispatched = std::cell::Cell::new(0);
    module
        .execute(
            &ctx(),
            CashuCompleteDepositAction {
                quote_id: "unknown-quote".to_string(),
            },
            "corr-1",
            &|_cmd| dispatched.set(dispatched.get() + 1),
        )
        .expect("execute must succeed (fail-closed commands still count as success)");
    assert!(
        dispatched.get() > 0,
        "fail-closed path must still emit a command"
    );
}
