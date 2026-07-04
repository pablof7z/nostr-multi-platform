//! Cross-mint transfer (#3003) resume state + source-mint selection — split
//! out of `state.rs` (AGENTS.md LOC discipline). See
//! `backend::cashu::cross_mint`'s module docs for the full melt -> mint saga
//! this state drives.

use super::StoredProof;
use crate::journal::WalletOperationId;
use nmp_nip60::cashu::types::Proof;
use nmp_signer_iface::SignedEvent;

/// A cross-mint transfer past its target mint-quote, keyed by
/// `target_quote_id` in `CashuWalletState::pending_cross_mint_transfers`.
/// Mirrors `PendingDeposit`'s field lifecycle (quote -> melt settled ->
/// minted -> signed) plus the melt leg's own `melt_settled` gate.
#[derive(Clone)]
pub(in crate::backend::cashu) struct PendingCrossMintTransfer {
    pub(in crate::backend::cashu) operation_id: WalletOperationId,
    pub(in crate::backend::cashu) target_mint: String,
    pub(in crate::backend::cashu) source_mint: String,
    pub(in crate::backend::cashu) amount_sats: u64,
    pub(in crate::backend::cashu) target_quote_id: String,
    /// The SOURCE mint's NUT-05 melt-quote id — recorded durably (WAL
    /// write-through) BEFORE the melt HTTP call goes out, alongside
    /// `source_selected`, per the money-safety invariant every melt in this
    /// codebase must satisfy.
    pub(in crate::backend::cashu) melt_quote_id: String,
    /// The exact source proofs reserved (removed from the spendable
    /// inventory the instant this is first recorded) for the melt. Restored
    /// only if the melt reconciles as definitely UNPAID/expired — never
    /// while `melt_settled` is `true` or unknown.
    pub(in crate::backend::cashu) source_selected: Vec<StoredProof>,
    /// `true` once the melt itself is confirmed PAID — either the live
    /// `melt()` call returned `PAID` directly, or a cold-restart resume's
    /// `get_melt_quote_status` reconcile confirmed it. This is the source
    /// leg's own irreversible-effect marker (the Lightning payment has left
    /// the source mint); `false` before that point means the melt's true
    /// outcome is still ambiguous and must be reconciled via
    /// `get_melt_quote_status`, never assumed.
    pub(in crate::backend::cashu) melt_settled: bool,
    /// Target-mint proofs, once NUT-04 `mint_tokens` actually returns them —
    /// write-if-absent (#2946), exactly mirroring
    /// `PendingDeposit::minted_proofs`: a Cashu mint marks a quote `ISSUED`
    /// the instant it hands out proofs and permanently refuses to re-mint
    /// it, so a retry must resume from these same proofs rather than
    /// re-minting.
    pub(in crate::backend::cashu) minted_proofs: Option<Vec<Proof>>,
    /// The signed kind:7375 for `minted_proofs`, once signing succeeds —
    /// mirrors `PendingDeposit::signed_token`: never re-sign, only re-publish
    /// this exact cached event on retry.
    pub(in crate::backend::cashu) signed_token: Option<SignedEvent>,
    /// Self-healing in-flight lease — mirrors
    /// `PendingDeposit::chain_started_at`'s doc comment exactly (same
    /// concurrent-retry double-fold hazard, same fix).
    pub(in crate::backend::cashu) chain_started_at: Option<u64>,
}

/// The mint — among every mint OTHER than `exclude` this wallet holds proofs
/// at — with the LARGEST total spendable balance, provided that total is at
/// least `min_amount`. Used to auto-select a cross-mint-transfer SOURCE
/// mint: the design picks the largest balance (not first-fit/insertion
/// order) as the strictly-safer choice for routing a small payment.
///
/// `min_amount` is a LOWER-BOUND proxy only — the real melt total, once the
/// target's bolt11 and the source melt-quote are both known, is
/// `melt_quote.amount + melt_quote.fee_reserve`, always >= the bare nutzap
/// amount. A mint that cannot even cover the bare amount can never cover the
/// real (fee-inclusive) total either, so this is safe to call before either
/// quote exists; the caller MUST re-verify against the real total via
/// `CashuWalletState::select_proofs` once the melt quote is known and fail
/// closed if it no longer covers it (balances can shift between this
/// candidate check and the actual reservation).
#[must_use]
pub(super) fn largest_spendable_mint_excluding(
    proofs: &[StoredProof],
    exclude: &str,
    min_amount: u64,
) -> Option<(String, u64)> {
    let excluded = super::canonicalize_mint_url(exclude);
    let mut totals: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    for stored in proofs {
        let mint = super::canonicalize_mint_url(&stored.mint);
        if mint == excluded {
            continue;
        }
        *totals.entry(mint).or_insert(0) += stored.proof.amount;
    }
    totals
        .into_iter()
        .filter(|(_, total)| *total >= min_amount)
        .max_by_key(|(_, total)| *total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_nip60::cashu::types::Proof;

    fn proof(mint: &str, amount: u64) -> StoredProof {
        StoredProof {
            token_event: None,
            mint: mint.to_string(),
            proof: Proof {
                amount,
                id: "00abc".to_string(),
                secret: format!("secret-{amount}"),
                c: "02".to_string() + &"aa".repeat(32),
                dleq: None,
                witness: None,
            },
        }
    }

    #[test]
    fn picks_largest_balance_excluding_target() {
        let proofs = vec![
            proof("https://mint-a.example", 10),
            proof("https://mint-b.example", 50),
            proof("https://mint-c.example", 30),
        ];
        // mint-b holds the largest balance overall, but it's the target —
        // must be excluded, leaving mint-c (30) as the largest remaining.
        let (mint, total) =
            largest_spendable_mint_excluding(&proofs, "https://mint-b.example", 1).unwrap();
        assert_eq!(mint, "https://mint-c.example");
        assert_eq!(total, 30);
    }

    #[test]
    fn returns_none_when_no_other_mint_meets_min_amount() {
        let proofs = vec![proof("https://mint-a.example", 5)];
        assert!(largest_spendable_mint_excluding(&proofs, "https://mint-b.example", 100).is_none());
    }

    #[test]
    fn sums_multiple_proofs_at_the_same_mint() {
        let proofs = vec![
            proof("https://mint-a.example", 4),
            proof("https://mint-a.example", 8),
        ];
        let (mint, total) =
            largest_spendable_mint_excluding(&proofs, "https://mint-b.example", 1).unwrap();
        assert_eq!(mint, "https://mint-a.example");
        assert_eq!(total, 12);
    }
}
