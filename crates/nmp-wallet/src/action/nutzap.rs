//! `nmp.wallet.nutzap.*` — W5 (#2908, epic #2864).
//!
//! Dispatch points, not finished flows: no registered backend advertises
//! `publish_nutzap_info`/`send_nutzap`/`redeem_nutzap` yet (NWC has no Cashu
//! concept; `CashuWalletBackend::capabilities()` is
//! `cashu_wallet_and_deposit()`, which deliberately does not bundle the
//! nutzap flags — see that constructor's doc comment). That makes these
//! three modules fail closed through the SAME generic
//! `require_capable_backend`/`selector.dispatch` path `action::cashu`'s
//! implemented actions use (shared via `action::mod`'s `require_capable_backend`)
//! — an absent capability, not a panic and not a special-cased rejection.
//! The moment a future wave (#2864 W8/W9/W13) makes a backend advertise one
//! of these flags, these modules start reaching it with no code change here.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use nmp_core::actor::ActorCommand;
use nmp_core::slots::ActiveAccountSlot;
use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection, DeclaredActionNamespace};

use crate::backend::WalletIntent;
use crate::selector::WalletBackendSelector;
use crate::{ACTION_NUTZAP_PUBLISH_INFO, ACTION_NUTZAP_REDEEM, ACTION_NUTZAP_SEND};

use super::{dispatch_and_forward, require_capable_backend};

// ── nmp.wallet.nutzap.publish_info ──────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct NutzapPublishInfoAction {}

pub struct NutzapPublishInfoModule {
    selector: Arc<WalletBackendSelector>,
    active_pubkey: ActiveAccountSlot,
}

impl NutzapPublishInfoModule {
    #[must_use]
    pub fn new(selector: Arc<WalletBackendSelector>, active_pubkey: ActiveAccountSlot) -> Self {
        Self {
            selector,
            active_pubkey,
        }
    }
}

impl ActionModule for NutzapPublishInfoModule {
    const NAMESPACE: DeclaredActionNamespace =
        DeclaredActionNamespace::framework(ACTION_NUTZAP_PUBLISH_INFO, "action.nmp.wallet.nutzap");

    type Action = NutzapPublishInfoAction;

    fn start(
        &self,
        _ctx: &mut ActionContext,
        _action: Self::Action,
    ) -> Result<(), ActionRejection> {
        require_capable_backend(&self.selector, &WalletIntent::PublishNutzapInfo)
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
            WalletIntent::PublishNutzapInfo,
            correlation_id,
            send,
        );
        Ok(())
    }
}

// ── nmp.wallet.nutzap.send ───────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct NutzapSendAction {
    pub recipient_pubkey: String,
    pub amount_sats: u64,
    pub target_event_id: Option<String>,
}

pub struct NutzapSendModule {
    selector: Arc<WalletBackendSelector>,
    active_pubkey: ActiveAccountSlot,
}

impl NutzapSendModule {
    #[must_use]
    pub fn new(selector: Arc<WalletBackendSelector>, active_pubkey: ActiveAccountSlot) -> Self {
        Self {
            selector,
            active_pubkey,
        }
    }
}

impl ActionModule for NutzapSendModule {
    const NAMESPACE: DeclaredActionNamespace =
        DeclaredActionNamespace::framework(ACTION_NUTZAP_SEND, "action.nmp.wallet.nutzap");

    type Action = NutzapSendAction;

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        if action.recipient_pubkey.trim().is_empty() {
            return Err(ActionRejection::Invalid(
                "nutzap.send requires a non-empty recipient_pubkey".to_string(),
            ));
        }
        if action.amount_sats == 0 {
            return Err(ActionRejection::Invalid(
                "nutzap.send requires amount_sats > 0".to_string(),
            ));
        }
        require_capable_backend(
            &self.selector,
            &WalletIntent::SendNutzap {
                recipient_pubkey: action.recipient_pubkey.clone(),
                amount_sats: action.amount_sats,
                target_event_id: action.target_event_id.clone(),
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
            WalletIntent::SendNutzap {
                recipient_pubkey: action.recipient_pubkey,
                amount_sats: action.amount_sats,
                target_event_id: action.target_event_id,
            },
            correlation_id,
            send,
        );
        Ok(())
    }
}

// ── nmp.wallet.nutzap.redeem ─────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct NutzapRedeemAction {
    pub event_id: String,
}

pub struct NutzapRedeemModule {
    selector: Arc<WalletBackendSelector>,
    active_pubkey: ActiveAccountSlot,
}

impl NutzapRedeemModule {
    #[must_use]
    pub fn new(selector: Arc<WalletBackendSelector>, active_pubkey: ActiveAccountSlot) -> Self {
        Self {
            selector,
            active_pubkey,
        }
    }
}

impl ActionModule for NutzapRedeemModule {
    const NAMESPACE: DeclaredActionNamespace =
        DeclaredActionNamespace::framework(ACTION_NUTZAP_REDEEM, "action.nmp.wallet.nutzap");

    type Action = NutzapRedeemAction;

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        if action.event_id.trim().is_empty() {
            return Err(ActionRejection::Invalid(
                "nutzap.redeem requires a non-empty event_id".to_string(),
            ));
        }
        require_capable_backend(
            &self.selector,
            &WalletIntent::RedeemNutzap {
                event_id: action.event_id.clone(),
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
            WalletIntent::RedeemNutzap {
                event_id: action.event_id,
            },
            correlation_id,
            send,
        );
        Ok(())
    }
}

#[cfg(test)]
#[path = "nutzap_tests.rs"]
mod tests;
