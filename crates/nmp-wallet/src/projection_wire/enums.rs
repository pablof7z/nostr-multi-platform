//! Enum bridges between the domain `Wallet*` enums and their generated
//! FlatBuffers counterparts.
//!
//! Extracted from `projection_wire.rs` to keep each file under the 500-LOC
//! hard cap.

use super::generated::nmp::wallet as fb;
use crate::journal::{WalletOperationKind, WalletOperationState};
use crate::projection::{WalletHistoryKind, WalletReadiness};

pub(super) fn readiness_to_fb(readiness: WalletReadiness) -> fb::WalletReadiness {
    match readiness {
        WalletReadiness::NotConfigured => fb::WalletReadiness::NotConfigured,
        WalletReadiness::Activating => fb::WalletReadiness::Activating,
        WalletReadiness::Ready => fb::WalletReadiness::Ready,
        WalletReadiness::Degraded => fb::WalletReadiness::Degraded,
    }
}

pub(super) fn readiness_from_fb(readiness: fb::WalletReadiness) -> Result<WalletReadiness, String> {
    match readiness {
        fb::WalletReadiness::NotConfigured => Ok(WalletReadiness::NotConfigured),
        fb::WalletReadiness::Activating => Ok(WalletReadiness::Activating),
        fb::WalletReadiness::Ready => Ok(WalletReadiness::Ready),
        fb::WalletReadiness::Degraded => Ok(WalletReadiness::Degraded),
        other => Err(format!("unknown WalletReadiness discriminant {}", other.0)),
    }
}

pub(super) fn history_kind_to_fb(kind: WalletHistoryKind) -> fb::WalletHistoryKind {
    match kind {
        WalletHistoryKind::Deposit => fb::WalletHistoryKind::Deposit,
        WalletHistoryKind::SendNutzap => fb::WalletHistoryKind::SendNutzap,
        WalletHistoryKind::RedeemNutzap => fb::WalletHistoryKind::RedeemNutzap,
        WalletHistoryKind::PayBolt11 => fb::WalletHistoryKind::PayBolt11,
    }
}

pub(super) fn history_kind_from_fb(
    kind: fb::WalletHistoryKind,
) -> Result<WalletHistoryKind, String> {
    match kind {
        fb::WalletHistoryKind::Deposit => Ok(WalletHistoryKind::Deposit),
        fb::WalletHistoryKind::SendNutzap => Ok(WalletHistoryKind::SendNutzap),
        fb::WalletHistoryKind::RedeemNutzap => Ok(WalletHistoryKind::RedeemNutzap),
        fb::WalletHistoryKind::PayBolt11 => Ok(WalletHistoryKind::PayBolt11),
        other => Err(format!(
            "unknown WalletHistoryKind discriminant {}",
            other.0
        )),
    }
}

pub(super) fn operation_kind_to_fb(kind: WalletOperationKind) -> fb::WalletOperationKind {
    match kind {
        WalletOperationKind::SelectBackend => fb::WalletOperationKind::SelectBackend,
        WalletOperationKind::PayBolt11 => fb::WalletOperationKind::PayBolt11,
        WalletOperationKind::CreateCashuWallet => fb::WalletOperationKind::CreateCashuWallet,
        WalletOperationKind::PublishNutzapInfo => fb::WalletOperationKind::PublishNutzapInfo,
        WalletOperationKind::SendNutzap => fb::WalletOperationKind::SendNutzap,
        WalletOperationKind::RedeemNutzap => fb::WalletOperationKind::RedeemNutzap,
        WalletOperationKind::DepositCashu => fb::WalletOperationKind::DepositCashu,
        WalletOperationKind::MeltCashu => fb::WalletOperationKind::MeltCashu,
        WalletOperationKind::SetCashuMints => fb::WalletOperationKind::SetCashuMints,
    }
}

pub(super) fn operation_kind_from_fb(
    kind: fb::WalletOperationKind,
) -> Result<WalletOperationKind, String> {
    match kind {
        fb::WalletOperationKind::SelectBackend => Ok(WalletOperationKind::SelectBackend),
        fb::WalletOperationKind::PayBolt11 => Ok(WalletOperationKind::PayBolt11),
        fb::WalletOperationKind::CreateCashuWallet => Ok(WalletOperationKind::CreateCashuWallet),
        fb::WalletOperationKind::PublishNutzapInfo => Ok(WalletOperationKind::PublishNutzapInfo),
        fb::WalletOperationKind::SendNutzap => Ok(WalletOperationKind::SendNutzap),
        fb::WalletOperationKind::RedeemNutzap => Ok(WalletOperationKind::RedeemNutzap),
        fb::WalletOperationKind::DepositCashu => Ok(WalletOperationKind::DepositCashu),
        fb::WalletOperationKind::MeltCashu => Ok(WalletOperationKind::MeltCashu),
        fb::WalletOperationKind::SetCashuMints => Ok(WalletOperationKind::SetCashuMints),
        other => Err(format!(
            "unknown WalletOperationKind discriminant {}",
            other.0
        )),
    }
}

pub(super) fn operation_state_to_fb(state: WalletOperationState) -> fb::WalletOperationState {
    match state {
        WalletOperationState::Draft => fb::WalletOperationState::Draft,
        WalletOperationState::Prepared => fb::WalletOperationState::Prepared,
        WalletOperationState::MintPending => fb::WalletOperationState::MintPending,
        WalletOperationState::MintSettled => fb::WalletOperationState::MintSettled,
        WalletOperationState::PublishPending => fb::WalletOperationState::PublishPending,
        WalletOperationState::Settled => fb::WalletOperationState::Settled,
        WalletOperationState::Unknown => fb::WalletOperationState::Unknown,
        WalletOperationState::Failed => fb::WalletOperationState::Failed,
    }
}

pub(super) fn operation_state_from_fb(
    state: fb::WalletOperationState,
) -> Result<WalletOperationState, String> {
    match state {
        fb::WalletOperationState::Draft => Ok(WalletOperationState::Draft),
        fb::WalletOperationState::Prepared => Ok(WalletOperationState::Prepared),
        fb::WalletOperationState::MintPending => Ok(WalletOperationState::MintPending),
        fb::WalletOperationState::MintSettled => Ok(WalletOperationState::MintSettled),
        fb::WalletOperationState::PublishPending => Ok(WalletOperationState::PublishPending),
        fb::WalletOperationState::Settled => Ok(WalletOperationState::Settled),
        fb::WalletOperationState::Unknown => Ok(WalletOperationState::Unknown),
        fb::WalletOperationState::Failed => Ok(WalletOperationState::Failed),
        other => Err(format!(
            "unknown WalletOperationState discriminant {}",
            other.0
        )),
    }
}
