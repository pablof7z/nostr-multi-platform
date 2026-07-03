//! `nmp.wallet.cashu.*` — W5 (#2908, epic #2864).
//!
//! `create`/`deposit_quote`/`complete_deposit` translate their typed payload
//! into the matching `WalletIntent` and dispatch through the W4 selector
//! (`action::dispatch_and_forward`), so they route to whichever registered
//! backend advertises the capability — today always `CashuWalletBackend`,
//! but the action modules themselves name no backend.
//!
//! `recover` is different: `WalletCapability::CreateCashuWallet` bundles
//! BOTH the `create` and `recover` action namespaces for UI-surfacing
//! purposes (`WalletCapabilities::action_namespaces`), so the generic
//! capability-resolution check the other three modules rely on cannot tell
//! "a backend can create" from "a backend can also recover" — and no backend
//! implements recovery yet (`CashuWalletBackend::start_intent` documents
//! `RecoverCashuWallet` as an out-of-scope no-op). `CashuRecoverModule`
//! therefore rejects unconditionally in `start()` rather than trusting
//! capability resolution to catch it — an honest, narrow special case, not a
//! silent no-op-that-looks-like-success.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use nmp_core::actor::ActorCommand;
use nmp_core::slots::ActiveAccountSlot;
use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection, DeclaredActionNamespace};

use crate::backend::WalletIntent;
use crate::selector::WalletBackendSelector;
use crate::ui_codes;
use crate::{
    ACTION_CASHU_COMPLETE_DEPOSIT, ACTION_CASHU_CREATE, ACTION_CASHU_DEPOSIT_QUOTE,
    ACTION_CASHU_RECOVER,
};

use super::dispatch_and_forward;

// ── nmp.wallet.cashu.create ─────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CashuCreateAction {
    pub mint: String,
}

pub struct CashuCreateModule {
    selector: Arc<WalletBackendSelector>,
    active_pubkey: ActiveAccountSlot,
}

impl CashuCreateModule {
    #[must_use]
    pub fn new(selector: Arc<WalletBackendSelector>, active_pubkey: ActiveAccountSlot) -> Self {
        Self {
            selector,
            active_pubkey,
        }
    }
}

impl ActionModule for CashuCreateModule {
    const NAMESPACE: DeclaredActionNamespace =
        DeclaredActionNamespace::framework(ACTION_CASHU_CREATE, "action.nmp.wallet.cashu");

    type Action = CashuCreateAction;

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        if action.mint.trim().is_empty() {
            return Err(ActionRejection::Invalid(
                "cashu.create requires a non-empty mint URL".to_string(),
            ));
        }
        require_capable_backend(
            &self.selector,
            &WalletIntent::CreateCashuWallet {
                mint: action.mint.clone(),
            },
        )
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        dispatch_and_forward(
            &self.selector,
            &self.active_pubkey,
            WalletIntent::CreateCashuWallet { mint: action.mint },
            correlation_id,
            send,
        );
        Ok(())
    }
}

// ── nmp.wallet.cashu.recover ─────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CashuRecoverAction {}

pub struct CashuRecoverModule;

impl CashuRecoverModule {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for CashuRecoverModule {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionModule for CashuRecoverModule {
    const NAMESPACE: DeclaredActionNamespace =
        DeclaredActionNamespace::framework(ACTION_CASHU_RECOVER, "action.nmp.wallet.cashu");

    type Action = CashuRecoverAction;

    /// See the module doc comment: no backend implements Cashu wallet
    /// recovery yet, and the generic capability check cannot distinguish
    /// this from `create`, so this rejects unconditionally.
    fn start(
        &self,
        _ctx: &mut ActionContext,
        _action: Self::Action,
    ) -> Result<(), ActionRejection> {
        Err(ActionRejection::InvalidCoded {
            code: ui_codes::CASHU_RECOVER_NOT_IMPLEMENTED,
            message: "wallet recovery is not implemented yet".to_string(),
        })
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        // Unreachable — `start()` always rejects.
        Err("cashu.recover is not implemented".to_string())
    }
}

// ── nmp.wallet.cashu.deposit_quote ───────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CashuDepositQuoteAction {
    pub mint: String,
    pub amount_sats: u64,
}

pub struct CashuDepositQuoteModule {
    selector: Arc<WalletBackendSelector>,
    active_pubkey: ActiveAccountSlot,
}

impl CashuDepositQuoteModule {
    #[must_use]
    pub fn new(selector: Arc<WalletBackendSelector>, active_pubkey: ActiveAccountSlot) -> Self {
        Self {
            selector,
            active_pubkey,
        }
    }
}

impl ActionModule for CashuDepositQuoteModule {
    const NAMESPACE: DeclaredActionNamespace =
        DeclaredActionNamespace::framework(ACTION_CASHU_DEPOSIT_QUOTE, "action.nmp.wallet.cashu");

    type Action = CashuDepositQuoteAction;

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        if action.mint.trim().is_empty() {
            return Err(ActionRejection::Invalid(
                "cashu.deposit_quote requires a non-empty mint URL".to_string(),
            ));
        }
        if action.amount_sats == 0 {
            return Err(ActionRejection::Invalid(
                "cashu.deposit_quote requires amount_sats > 0".to_string(),
            ));
        }
        require_capable_backend(
            &self.selector,
            &WalletIntent::DepositQuoteCashu {
                mint: action.mint.clone(),
                amount_sats: action.amount_sats,
            },
        )
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        dispatch_and_forward(
            &self.selector,
            &self.active_pubkey,
            WalletIntent::DepositQuoteCashu {
                mint: action.mint,
                amount_sats: action.amount_sats,
            },
            correlation_id,
            send,
        );
        Ok(())
    }
}

// ── nmp.wallet.cashu.complete_deposit ────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CashuCompleteDepositAction {
    pub quote_id: String,
}

pub struct CashuCompleteDepositModule {
    selector: Arc<WalletBackendSelector>,
    active_pubkey: ActiveAccountSlot,
}

impl CashuCompleteDepositModule {
    #[must_use]
    pub fn new(selector: Arc<WalletBackendSelector>, active_pubkey: ActiveAccountSlot) -> Self {
        Self {
            selector,
            active_pubkey,
        }
    }
}

impl ActionModule for CashuCompleteDepositModule {
    const NAMESPACE: DeclaredActionNamespace = DeclaredActionNamespace::framework(
        ACTION_CASHU_COMPLETE_DEPOSIT,
        "action.nmp.wallet.cashu",
    );

    type Action = CashuCompleteDepositAction;

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        if action.quote_id.trim().is_empty() {
            return Err(ActionRejection::Invalid(
                "cashu.complete_deposit requires a non-empty quote_id".to_string(),
            ));
        }
        require_capable_backend(
            &self.selector,
            &WalletIntent::CompleteDepositCashu {
                quote_id: action.quote_id.clone(),
            },
        )
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        dispatch_and_forward(
            &self.selector,
            &self.active_pubkey,
            WalletIntent::CompleteDepositCashu {
                quote_id: action.quote_id,
            },
            correlation_id,
            send,
        );
        Ok(())
    }
}

/// Shared `start()` gate: reject before dispatch when zero registered
/// backends could ever satisfy `intent`'s capability — absent capability is
/// a structured rejection, never a silent no-op reached via `execute()`.
fn require_capable_backend(
    selector: &WalletBackendSelector,
    intent: &WalletIntent,
) -> Result<(), ActionRejection> {
    let Some(capability) = crate::selector::capability_for(intent) else {
        return Ok(());
    };
    if selector.candidates_for(capability).is_empty() {
        return Err(ActionRejection::InvalidCoded {
            code: ui_codes::NO_CAPABLE_BACKEND,
            message: "no registered wallet backend supports this operation".to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
#[path = "cashu_tests.rs"]
mod tests;
