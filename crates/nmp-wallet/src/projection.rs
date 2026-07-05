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
    /// The counterparty pubkey, when one exists (`RedeemNutzap` only as of
    /// #2966 — `None` for `Deposit`/`SendNutzap`, which have no external
    /// sender to name).
    pub sender: Option<String>,
    /// When this operation was recorded, in unix seconds (#2966) — `None`
    /// only for a row folded from state that predates this field.
    pub timestamp: Option<u64>,
    pub state: String,
    /// The mint a `SendNutzap` ultimately drew its Lightning-melt funding
    /// from, when it took the cross-mint auto-fallback (#3003/#3008) —
    /// `None` for an intra-mint send (where it equals `target_mint`) or any
    /// other history kind. Lets a shell render "sent via mint A -> mint B"
    /// without ever decoding a proof.
    pub source_mint: Option<String>,
    /// The mint a `SendNutzap` actually sent the nutzap FROM — the same
    /// mint `consumed_inputs` names (#3008). `None` only for a send that
    /// never reached proof selection (failed before a mint was chosen).
    pub target_mint: Option<String>,
    /// The total fee, in sats, this `SendNutzap` actually cost: its own
    /// P2PK swap fee at `target_mint`, plus (for a cross-mint payment) the
    /// realized melt fee consumed funding `target_mint` from `source_mint`
    /// (#3008). `None` only for a send that never reached the mint swap.
    pub fee_paid_sats: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WalletReceiveRow {
    pub event_id: String,
    pub mint: String,
    pub amount: u64,
    pub unit: String,
    /// The kind:9321 nutzap's sender pubkey (#2966) — a nutzap feed's "from
    /// <pubkey>". `None` only if the operation predates this field.
    pub sender: Option<String>,
    /// When this operation was recorded, in unix seconds (#2966).
    pub timestamp: Option<u64>,
    pub accepted: bool,
}

/// One ranked NIP-87 discovered mint (#2880, epic #2864), mirroring the
/// display-safe fields of [`crate::mint_discovery::DiscoveredMint`]. Carries
/// no proofs, secrets, or keys — `DiscoveredMint` has none to begin with;
/// this is pure discovery metadata a shell can render as a mint explorer /
/// "discover mints" screen.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct WalletDiscoveredMint {
    /// Mint URL (the value a consumer selects instead of a hardcoded entry).
    pub url: String,
    /// Human-readable name, if any announcement advertised one.
    pub name: Option<String>,
    /// Supported NUT numbers (union across announcements for this URL).
    pub nuts: Vec<u16>,
    /// Supported units, when advertised.
    pub units: Vec<String>,
    /// True when the mint advertises the nutzap-required NUTs.
    pub supports_nutzap: bool,
    /// Summed web-of-trust score of the distinct trusted recommenders.
    pub trust_score: i32,
    /// Count of distinct trusted recommenders.
    pub recommendation_count: u32,
}

impl From<&crate::mint_discovery::DiscoveredMint> for WalletDiscoveredMint {
    fn from(mint: &crate::mint_discovery::DiscoveredMint) -> Self {
        Self {
            url: mint.url.clone(),
            name: mint.name.clone(),
            nuts: mint.nuts.clone(),
            units: mint.units.clone(),
            supports_nutzap: mint.supports_nutzap,
            trust_score: mint.trust_score,
            recommendation_count: mint.recommendation_count,
        }
    }
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
    /// Ranked, capability-filtered, web-of-trust-scoped NIP-87 discovered
    /// mints (#2880, epic #2864) — see [`WalletDiscoveredMint`].
    pub discovered_mints: Vec<WalletDiscoveredMint>,
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

    /// Set the discovered-mints view, bounded to
    /// `mint_discovery::MAX_DISCOVERED_MINTS`. Unlike the recency-ordered rows
    /// above (`bounded` keeps the newest tail), `mints` is already ranked
    /// best-first by [`crate::mint_discovery::aggregate_discovered_mints`], so
    /// an over-cap input is truncated from the tail (dropping the
    /// lowest-ranked entries), keeping the highest-ranked prefix.
    #[must_use]
    pub fn with_discovered_mints(
        mut self,
        mints: impl IntoIterator<Item = WalletDiscoveredMint>,
    ) -> Self {
        let mut mints: Vec<WalletDiscoveredMint> = mints.into_iter().collect();
        mints.truncate(crate::mint_discovery::MAX_DISCOVERED_MINTS);
        self.discovered_mints = mints;
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
            sender: None,
            timestamp: None,
            state: "settled".to_string(),
            source_mint: None,
            target_mint: None,
            fee_paid_sats: None,
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
