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

/// Known **valueless / non-Lightning-settleable** mints, excluded from
/// cross-mint SOURCE selection (#3010). These are public Cashu *test* mints
/// (canonically the `testnut.cashu.space` family) whose Lightning backend is
/// a fake/regtest wallet: they hand out ecash for free (so a wallet can
/// accumulate a large "balance" at them), but they CANNOT settle a real
/// mainnet bolt11 — a melt to pay a real target invoice goes `PENDING` and
/// never completes.
///
/// Why a denylist and not a runtime probe: such a mint is *protocol-
/// indistinguishable* from a real one before the irreversible melt. It
/// returns a perfectly valid melt QUOTE for the real target invoice (so a
/// pre-melt quote probe cannot catch it), and after `melt()` it reports
/// `PENDING`, not a definite `UNPAID` — and money-safety (see
/// `cross_mint_worker`) forbids advancing past an ambiguous/pending melt.
/// The only point at which a fake test mint can be kept out of a real
/// Lightning melt is therefore SELECTION, and the only signal available at
/// selection time is the mint's identity. The list is deliberately tiny and
/// matches the well-known canonical test-mint host family; unknown mints that
/// *definitely* fail are instead handled by the caller's next-candidate
/// fall-through (a pre-melt quote/keyset/reserve failure moves no funds, so
/// the worker simply tries the next source).
#[must_use]
pub(super) fn is_known_valueless_mint(mint: &str) -> bool {
    // Match on the canonical authority (host[:port]) only — a mint's path is
    // load-bearing (`canonicalize_mint_url`), but "is this the testnut fake
    // mint" is a property of the HOST, independent of any path a test mint
    // serves. Covers both `testnut.cashu.space` and its published
    // subdomains (e.g. `nofees.testnut.cashu.space`).
    let canonical = super::canonicalize_mint_url(mint);
    let Some((_scheme, rest)) = canonical.split_once("://") else {
        return false;
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = rest[..authority_end]
        .rsplit_once(':')
        .map_or(&rest[..authority_end], |(h, _port)| h);
    host == "testnut.cashu.space" || host.ends_with(".testnut.cashu.space")
}

/// Every mint this wallet could MELT from to fund a cross-mint transfer to
/// `exclude` (#3003/#3010), as `(canonical_mint_url, spendable_total)` pairs
/// ordered by spendable balance **descending** (ties broken by canonical URL
/// for a deterministic order). A mint is a candidate iff it is NOT the target
/// (`exclude`), NOT a [known valueless mint](is_known_valueless_mint), and
/// holds at least `min_amount` spendable.
///
/// `min_amount` is a LOWER-BOUND proxy only — the real melt total, once the
/// target's bolt11 and the source melt-quote are both known, is
/// `melt_quote.amount + melt_quote.fee_reserve`, always >= the bare nutzap
/// amount. A mint that cannot even cover the bare amount can never cover the
/// real (fee-inclusive) total either, so this is safe to call before either
/// quote exists. The worker walks these candidates in order and, for each,
/// re-verifies against the real total via `CashuWalletState::select_proofs`
/// once the melt quote is known; a candidate that can't cover the
/// fee-inclusive total (or whose melt-quote/keyset fetch fails) is skipped in
/// favour of the next — all of which move no funds (#3010).
#[must_use]
pub(super) fn spendable_source_candidates_excluding(
    proofs: &[StoredProof],
    exclude: &str,
    min_amount: u64,
) -> Vec<(String, u64)> {
    let excluded = super::canonicalize_mint_url(exclude);
    let mut totals: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    for stored in proofs {
        let mint = super::canonicalize_mint_url(&stored.mint);
        if mint == excluded || is_known_valueless_mint(&mint) {
            continue;
        }
        *totals.entry(mint).or_insert(0) += stored.proof.amount;
    }
    let mut candidates: Vec<(String, u64)> = totals
        .into_iter()
        .filter(|(_, total)| *total >= min_amount)
        .collect();
    // Largest balance first (the strictly-safer choice for routing a small
    // payment); canonical URL as a stable tie-breaker so ordering is
    // deterministic across runs.
    candidates.sort_by(|(a_mint, a_total), (b_mint, b_total)| {
        b_total.cmp(a_total).then_with(|| a_mint.cmp(b_mint))
    });
    candidates
}

/// The single largest-balance cross-mint SOURCE candidate (or `None` if
/// there is no eligible mint) — a thin convenience over
/// [`spendable_source_candidates_excluding`] used by `send.rs`'s read-only
/// "is this target fundable at all?" probe. Shares the exact same eligibility
/// rules, so a target that is *only* fundable by a valueless test mint (which
/// this excludes) is correctly treated as unfundable (#3010).
#[must_use]
pub(super) fn largest_spendable_mint_excluding(
    proofs: &[StoredProof],
    exclude: &str,
    min_amount: u64,
) -> Option<(String, u64)> {
    spendable_source_candidates_excluding(proofs, exclude, min_amount)
        .into_iter()
        .next()
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

    #[test]
    fn identifies_the_testnut_fake_mint_family() {
        assert!(is_known_valueless_mint("https://testnut.cashu.space"));
        assert!(is_known_valueless_mint("https://testnut.cashu.space/"));
        assert!(is_known_valueless_mint("https://nofees.testnut.cashu.space"));
        // Case-insensitive host, path is irrelevant to the identity.
        assert!(is_known_valueless_mint("https://TestNut.Cashu.Space/Bitcoin"));
        // Real mints are never flagged.
        assert!(!is_known_valueless_mint("https://mint.minibits.cash/Bitcoin"));
        assert!(!is_known_valueless_mint("https://mint-a.example"));
        // A different host that merely CONTAINS the substring must not match.
        assert!(!is_known_valueless_mint(
            "https://testnut.cashu.space.evil.example"
        ));
    }

    #[test]
    fn candidates_exclude_valueless_test_mints_even_when_largest() {
        // The WAL scenario (#3010): the LARGEST balance sits at the valueless
        // testnut fake mint, with ample REAL balance at other mints. Source
        // selection must skip testnut entirely and rank only the settleable
        // mints — largest real balance first.
        let proofs = vec![
            proof("https://testnut.cashu.space", 1200),
            proof("https://mint.chorus.example", 300),
            proof("https://mint.flashapp.example", 500),
            proof("https://mint.minibits.cash/Bitcoin", 100),
        ];
        let candidates = spendable_source_candidates_excluding(
            &proofs,
            "https://mint.minibits.cash/Bitcoin", // target — also excluded
            50,
        );
        assert_eq!(
            candidates,
            vec![
                ("https://mint.flashapp.example".to_string(), 500),
                ("https://mint.chorus.example".to_string(), 300),
            ],
            "testnut (largest) and the target must be excluded; real mints ranked by balance desc"
        );
        // The single-pick convenience never returns the valueless mint either.
        let (mint, total) =
            largest_spendable_mint_excluding(&proofs, "https://mint.minibits.cash/Bitcoin", 50)
                .unwrap();
        assert_eq!(mint, "https://mint.flashapp.example");
        assert_eq!(total, 500);
    }

    #[test]
    fn candidates_are_none_when_only_a_valueless_mint_could_fund() {
        // testnut holds plenty, but it's the ONLY non-target mint — there is
        // no settleable source, so the transfer must fail closed rather than
        // stall on an unsettleable melt.
        let proofs = vec![
            proof("https://testnut.cashu.space", 1200),
            proof("https://mint.minibits.cash/Bitcoin", 100),
        ];
        let candidates = spendable_source_candidates_excluding(
            &proofs,
            "https://mint.minibits.cash/Bitcoin",
            50,
        );
        assert!(candidates.is_empty());
        assert!(
            largest_spendable_mint_excluding(&proofs, "https://mint.minibits.cash/Bitcoin", 50)
                .is_none()
        );
    }

    #[test]
    fn candidates_ranked_by_balance_desc_with_stable_tie_break() {
        let proofs = vec![
            proof("https://mint-c.example", 30),
            proof("https://mint-a.example", 30),
            proof("https://mint-b.example", 50),
        ];
        let candidates =
            spendable_source_candidates_excluding(&proofs, "https://target.example", 1);
        assert_eq!(
            candidates,
            vec![
                ("https://mint-b.example".to_string(), 50),
                // 30-vs-30 tie broken by canonical URL ascending.
                ("https://mint-a.example".to_string(), 30),
                ("https://mint-c.example".to_string(), 30),
            ]
        );
    }
}
