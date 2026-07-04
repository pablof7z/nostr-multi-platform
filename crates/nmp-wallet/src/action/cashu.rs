//! `nmp.wallet.cashu.*` — W5 (#2908, epic #2864).
//!
//! `create`/`recover`/`deposit_quote`/`complete_deposit`/`set_mints` all
//! translate their typed payload into the matching `WalletIntent` and
//! dispatch through the W4 selector (`action::dispatch_and_forward`), so they
//! route to whichever registered backend advertises the capability — today
//! always `CashuWalletBackend`, but the action modules themselves name no
//! backend.
//!
//! `recover` (#2965, epic #2864) loads an account's EXISTING on-relay
//! kind:17375 wallet config into state rather than minting fresh —
//! `CashuWalletBackend::start_intent` now implements `RecoverCashuWallet` via
//! `recover::RecoverCashuWalletCommand`. `WalletCapability::CreateCashuWallet`
//! still bundles the `create`/`recover`/`set_mints` action namespaces for
//! UI-surfacing purposes (`WalletCapabilities::action_namespaces`), which is
//! fine here: all three namespaces map to the SAME capability a backend
//! either has or doesn't, and `CashuWalletBackend` now implements all three.
//!
//! `set_mints` (#2997, epic #2864) is the key-PRESERVING counterpart to
//! `create`: it replaces the wallet's accepted-mint list without rotating
//! the Cashu P2PK receive key `create`'s `WalletConfig::generate` mints fresh
//! every call — see `backend::cashu::set_mints::SetCashuMintsCommand`'s
//! module docs for why that distinction is money-safety-critical (rotating
//! the key would strand already-incoming P2PK-locked proofs).
//!
//! `deposit_quote`/`complete_deposit` live in the sibling `cashu_deposit.rs`
//! (AGENTS.md LOC discipline) and are re-exported here so this file stays
//! the one `use crate::action::cashu::*`-style seam for the whole family.

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
#[cfg(test)]
use crate::ui_codes;
use crate::{
    ACTION_CASHU_CREATE, ACTION_CASHU_CROSS_MINT_TRANSFER, ACTION_CASHU_RECOVER,
    ACTION_CASHU_SET_MINTS,
};

use super::{dispatch_and_forward, require_capable_backend};

#[path = "cashu_deposit.rs"]
mod cashu_deposit;
pub use cashu_deposit::{
    CashuCompleteDepositAction, CashuCompleteDepositModule, CashuDepositQuoteAction,
    CashuDepositQuoteModule,
};

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

    /// Typed FlatBuffers payload decode (ADR-0071 / #2920) — delegates to the
    /// `nmp.wallet.cashu.create` `ActionPayload` codec (`NWCC`). The registry
    /// adapter runs the fail-closed `schema_version` gate BEFORE `start()`.
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<Self::Action as ActionPayload>::decode(bytes))
    }

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

pub struct CashuRecoverModule {
    selector: Arc<WalletBackendSelector>,
    active_pubkey: ActiveAccountSlot,
}

impl CashuRecoverModule {
    #[must_use]
    pub fn new(selector: Arc<WalletBackendSelector>, active_pubkey: ActiveAccountSlot) -> Self {
        Self {
            selector,
            active_pubkey,
        }
    }
}

impl ActionModule for CashuRecoverModule {
    const NAMESPACE: DeclaredActionNamespace =
        DeclaredActionNamespace::framework(ACTION_CASHU_RECOVER, "action.nmp.wallet.cashu");

    type Action = CashuRecoverAction;

    /// Typed FlatBuffers payload decode (ADR-0071 / #2920) — delegates to the
    /// `nmp.wallet.cashu.recover` `ActionPayload` codec (`NWCR`). The registry
    /// adapter runs the fail-closed `schema_version` gate BEFORE `start()`.
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<Self::Action as ActionPayload>::decode(bytes))
    }

    fn start(
        &self,
        _ctx: &mut ActionContext,
        _action: Self::Action,
    ) -> Result<(), ActionRejection> {
        require_capable_backend(&self.selector, &WalletIntent::RecoverCashuWallet)
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        _action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        dispatch_and_forward(
            &self.selector,
            &self.active_pubkey,
            WalletIntent::RecoverCashuWallet,
            correlation_id,
            send,
        );
        Ok(())
    }
}

// ── nmp.wallet.cashu.set_mints ───────────────────────────────────────────────

/// #2997 — key-preserving wallet config edit: replaces the wallet's
/// accepted-mint list, carrying the existing Cashu P2PK privkey forward
/// unchanged. Unlike `cashu.create`, this NEVER mints a fresh privkey — see
/// `backend::cashu::set_mints::SetCashuMintsCommand`'s module docs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CashuSetMintsAction {
    pub mints: Vec<String>,
}

pub struct CashuSetMintsModule {
    selector: Arc<WalletBackendSelector>,
    active_pubkey: ActiveAccountSlot,
}

impl CashuSetMintsModule {
    #[must_use]
    pub fn new(selector: Arc<WalletBackendSelector>, active_pubkey: ActiveAccountSlot) -> Self {
        Self {
            selector,
            active_pubkey,
        }
    }
}

impl ActionModule for CashuSetMintsModule {
    const NAMESPACE: DeclaredActionNamespace =
        DeclaredActionNamespace::framework(ACTION_CASHU_SET_MINTS, "action.nmp.wallet.cashu");

    type Action = CashuSetMintsAction;

    /// Typed FlatBuffers payload decode (ADR-0071 / #2920) — delegates to the
    /// `nmp.wallet.cashu.set_mints` `ActionPayload` codec (`NWSM`). The
    /// registry adapter runs the fail-closed `schema_version` gate BEFORE
    /// `start()`.
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<Self::Action as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        if action.mints.is_empty() {
            return Err(ActionRejection::Invalid(
                "cashu.set_mints requires a non-empty mint list".to_string(),
            ));
        }
        if let Some(bad) = action
            .mints
            .iter()
            .find(|m| !crate::backend::cashu::is_well_formed_mint_url(m))
        {
            return Err(ActionRejection::Invalid(format!(
                "cashu.set_mints requires every mint to be a well-formed URL, got: {bad}"
            )));
        }
        require_capable_backend(
            &self.selector,
            &WalletIntent::SetCashuMints {
                mints: action.mints.clone(),
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
            WalletIntent::SetCashuMints {
                mints: action.mints,
            },
            correlation_id,
            send,
        );
        Ok(())
    }
}

// ── nmp.wallet.cashu.cross_mint_transfer ─────────────────────────────────────

/// #3003 — cross-mint nutzap funding: fund `target_mint` with `amount_sats`
/// by melting proofs at whichever OTHER mint holds the largest spendable
/// balance, minting SELF-owned proofs at the target and publishing them as
/// kind:7375. See `backend::cashu::cross_mint`'s module docs for the full
/// melt -> mint saga. The `nutzap.send` auto-fallback drives the same saga
/// internally without this action; this exists for explicit/POC driving.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CashuCrossMintTransferAction {
    pub target_mint: String,
    pub amount_sats: u64,
}

pub struct CashuCrossMintTransferModule {
    selector: Arc<WalletBackendSelector>,
    active_pubkey: ActiveAccountSlot,
}

impl CashuCrossMintTransferModule {
    #[must_use]
    pub fn new(selector: Arc<WalletBackendSelector>, active_pubkey: ActiveAccountSlot) -> Self {
        Self {
            selector,
            active_pubkey,
        }
    }
}

impl ActionModule for CashuCrossMintTransferModule {
    const NAMESPACE: DeclaredActionNamespace = DeclaredActionNamespace::framework(
        ACTION_CASHU_CROSS_MINT_TRANSFER,
        "action.nmp.wallet.cashu",
    );

    type Action = CashuCrossMintTransferAction;

    /// Typed FlatBuffers payload decode (ADR-0071 / #2920) — delegates to
    /// the `nmp.wallet.cashu.cross_mint_transfer` `ActionPayload` codec
    /// (`NWCX`). The registry adapter runs the fail-closed `schema_version`
    /// gate BEFORE `start()`.
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<Self::Action as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        if action.amount_sats == 0 {
            return Err(ActionRejection::Invalid(
                "cross_mint_transfer requires amount_sats > 0".to_string(),
            ));
        }
        if !crate::backend::cashu::is_well_formed_mint_url(&action.target_mint) {
            return Err(ActionRejection::Invalid(format!(
                "cross_mint_transfer requires a well-formed target mint URL, got: {}",
                action.target_mint
            )));
        }
        require_capable_backend(
            &self.selector,
            &WalletIntent::CrossMintTransfer {
                target_mint: action.target_mint.clone(),
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
            WalletIntent::CrossMintTransfer {
                target_mint: action.target_mint,
                amount_sats: action.amount_sats,
            },
            correlation_id,
            send,
        );
        Ok(())
    }
}

#[cfg(test)]
#[path = "cashu_tests.rs"]
mod tests;
