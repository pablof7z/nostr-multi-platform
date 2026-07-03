use serde::{Deserialize, Serialize};

use crate::{WalletBackendId, WalletCapabilities, WalletOperation};

pub const WALLET_PROJECTION_KEY: &str = "wallet";
pub const MAX_WALLET_PROJECTION_ROWS: usize = 100;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum WalletReadiness {
    #[default]
    NotConfigured,
    Activating,
    Ready,
    Degraded,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WalletBalanceRow {
    pub mint: String,
    pub unit: String,
    pub amount: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WalletHistoryKind {
    Deposit,
    SendNutzap,
    RedeemNutzap,
    PayBolt11,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WalletHistoryRow {
    pub operation_id: String,
    pub kind: WalletHistoryKind,
    pub amount: u64,
    pub unit: String,
    pub state: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WalletReceiveRow {
    pub event_id: String,
    pub mint: String,
    pub amount: u64,
    pub unit: String,
    pub accepted: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct WalletProjection {
    pub active_backend_id: Option<WalletBackendId>,
    pub readiness: WalletReadiness,
    pub capabilities: WalletCapabilities,
    pub balances: Vec<WalletBalanceRow>,
    pub cashu_p2pk_pubkey: Option<String>,
    pub accepted_mint_count: u32,
    pub accepted_relay_count: u32,
    pub pending_operations: Vec<WalletOperation>,
    pub recent_history: Vec<WalletHistoryRow>,
    pub receive_rows: Vec<WalletReceiveRow>,
}

impl WalletProjection {
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn new(
        active_backend_id: Option<WalletBackendId>,
        readiness: WalletReadiness,
        capabilities: WalletCapabilities,
    ) -> Self {
        Self {
            active_backend_id,
            readiness,
            capabilities,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn with_balances(mut self, balances: impl IntoIterator<Item = WalletBalanceRow>) -> Self {
        self.balances = bounded(balances);
        self
    }

    #[must_use]
    pub fn with_pending_operations(
        mut self,
        operations: impl IntoIterator<Item = WalletOperation>,
    ) -> Self {
        self.pending_operations = bounded(operations);
        self
    }

    #[must_use]
    pub fn with_recent_history(
        mut self,
        history: impl IntoIterator<Item = WalletHistoryRow>,
    ) -> Self {
        self.recent_history = bounded(history);
        self
    }

    #[must_use]
    pub fn with_receive_rows(mut self, rows: impl IntoIterator<Item = WalletReceiveRow>) -> Self {
        self.receive_rows = bounded(rows);
        self
    }
}

fn bounded<T>(items: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut items: Vec<T> = items.into_iter().collect();
    if items.len() > MAX_WALLET_PROJECTION_ROWS {
        let keep_from = items.len() - MAX_WALLET_PROJECTION_ROWS;
        items.drain(..keep_from);
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{WalletOperationId, WalletOperationKind, WalletOperationState};

    #[test]
    fn projection_rows_are_bounded_to_recent_rows() {
        let history = (0..150).map(|idx| WalletHistoryRow {
            operation_id: format!("op-{idx}"),
            kind: WalletHistoryKind::Deposit,
            amount: idx,
            unit: "sat".to_string(),
            state: "settled".to_string(),
        });

        let projection = WalletProjection::empty().with_recent_history(history);

        assert_eq!(projection.recent_history.len(), MAX_WALLET_PROJECTION_ROWS);
        assert_eq!(projection.recent_history[0].operation_id, "op-50");
    }

    #[test]
    fn projection_never_requires_secret_wallet_material() {
        let op = WalletOperation::new(
            WalletOperationId::new("op-visible"),
            WalletOperationKind::RedeemNutzap,
            WalletOperationState::PublishPending,
        );
        let projection = WalletProjection::empty().with_pending_operations([op]);
        let json = serde_json::to_string(&projection).expect("projection serializes");

        for forbidden in ["proof", "secret", "quote_id", "nsec", "plaintext"] {
            assert!(
                !json.contains(forbidden),
                "projection JSON leaked forbidden field marker {forbidden}: {json}"
            );
        }
    }
}
