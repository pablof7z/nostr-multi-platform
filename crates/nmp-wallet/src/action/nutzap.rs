//! `nmp.wallet.nutzap.*` — W5 (#2908, epic #2864).
//!
//! Dispatch-only, same shape as `action::cashu`'s implemented actions
//! (shared `require_capable_backend`/`dispatch_and_forward` via
//! `action::mod`): validate the typed payload in `start()`, translate it to a
//! `WalletIntent`, and route it through the selector in `execute()`. NWC
//! never advertises any of the three capabilities (no Cashu concept); the
//! Cashu backend advertises all three as of #2917
//! (`CashuWalletBackend::capabilities()` = `cashu_nutzaps()`) and implements
//! them in `backend::cashu::{publish_info,send,redeem}`. Absent capability
//! (a backend that doesn't implement one of these) is a structured
//! `require_capable_backend` rejection, never a panic and never a
//! special-cased path here.

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

    /// Typed FlatBuffers payload decode (ADR-0071 / #2920) — delegates to the
    /// `nmp.wallet.nutzap.publish_info` `ActionPayload` codec (`NWPI`). The
    /// registry adapter runs the fail-closed `schema_version` gate BEFORE
    /// `start()`.
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<Self::Action as ActionPayload>::decode(bytes))
    }

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

    /// Typed FlatBuffers payload decode (ADR-0071 / #2920) — delegates to the
    /// `nmp.wallet.nutzap.send` `ActionPayload` codec (`NWNS`). The registry
    /// adapter runs the fail-closed `schema_version` gate BEFORE `start()`.
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<Self::Action as ActionPayload>::decode(bytes))
    }

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

    /// Typed FlatBuffers payload decode (ADR-0071 / #2920) — delegates to the
    /// `nmp.wallet.nutzap.redeem` `ActionPayload` codec (`NWNR`). The registry
    /// adapter runs the fail-closed `schema_version` gate BEFORE `start()`.
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<Self::Action as ActionPayload>::decode(bytes))
    }

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
