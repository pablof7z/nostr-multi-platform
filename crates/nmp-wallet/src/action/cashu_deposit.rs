//! `nmp.wallet.cashu.deposit_quote`/`complete_deposit` — split out of
//! `cashu.rs` (AGENTS.md LOC discipline). See that file's module docs for
//! the shared `nmp.wallet.cashu.*` family overview.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use nmp_core::actor::ActorCommand;
use nmp_core::slots::ActiveAccountSlot;
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRejection,
    DeclaredActionNamespace,
};

use crate::backend::WalletIntent;
use crate::selector::WalletBackendSelector;
use crate::{ACTION_CASHU_COMPLETE_DEPOSIT, ACTION_CASHU_DEPOSIT_QUOTE};

use super::{dispatch_and_forward, require_capable_backend};

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

    /// Typed FlatBuffers payload decode (ADR-0071 / #2920) — delegates to the
    /// `nmp.wallet.cashu.deposit_quote` `ActionPayload` codec (`NWDQ`). The
    /// registry adapter runs the fail-closed `schema_version` gate BEFORE
    /// `start()`.
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<Self::Action as ActionPayload>::decode(bytes))
    }

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

    /// Typed FlatBuffers payload decode (ADR-0071 / #2920) — delegates to the
    /// `nmp.wallet.cashu.complete_deposit` `ActionPayload` codec (`NWCD`). The
    /// registry adapter runs the fail-closed `schema_version` gate BEFORE
    /// `start()`.
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<Self::Action as ActionPayload>::decode(bytes))
    }

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
