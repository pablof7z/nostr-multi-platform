//! `PendingDeposit` — split out of `state.rs` (AGENTS.md LOC discipline).

use nmp_nip60::cashu::types::Proof;
use nmp_signer_iface::SignedEvent;

use crate::journal::WalletOperationId;

/// A deposit whose quote has been requested but not yet completed —
/// `crate::backend::WalletIntent::CompleteDepositCashu` looks operations up
/// by `quote_id` (the caller's only handle on a pending deposit; see the
/// action result surfacing in `deposit.rs`).
#[derive(Clone)]
pub(in crate::backend::cashu) struct PendingDeposit {
    pub(in crate::backend::cashu) operation_id: WalletOperationId,
    pub(in crate::backend::cashu) mint: String,
    pub(in crate::backend::cashu) amount_sats: u64,
    /// Set once NUT-04 `mint_tokens` has actually returned these proofs. A
    /// Cashu mint marks a quote `ISSUED` the moment it hands out proofs for
    /// it and permanently refuses to mint that quote again — so if the
    /// encrypt/sign/publish chain that follows fails while the process is
    /// still running (a transient port error, a dead-but-still-alive actor
    /// inbox), re-running `mint_tokens` on retry would either be rejected
    /// outright or silently forfeit these already-real proofs.
    /// `CashuCompleteDepositCommand` checks this FIRST, before touching the
    /// mint again, and resumes the chain with these same proofs when set
    /// (see `deposit.rs`'s module docs).
    ///
    /// PR-2 of #2910 made this durable: the instant these proofs are recorded
    /// (in `deposit/complete.rs`), they are also written to the pre-publish WAL
    /// as a `CashuWalPayload::Deposit` (`wal_payload.rs`), and
    /// `restore_from_wal` rebuilds this `PendingDeposit` from that payload on
    /// restart. A hard crash in the window between `mint_tokens` returning `Ok`
    /// and the kind:7375 event publishing therefore no longer loses these
    /// already-`ISSUED` proofs — a restarted process resumes the
    /// encrypt/sign/publish chain from them (see `ResumeDepositCommand`) rather
    /// than stranding real sats behind an `UNKNOWN_QUOTE`.
    ///
    /// Never cleared once set (mirrors `signed_token`): kept around both for
    /// sequential retries AND as the reference proof set `still_held`
    /// (`deposit.rs`) checks a cached `signed_token` against before
    /// republishing it.
    pub(in crate::backend::cashu) minted_proofs: Option<Vec<Proof>>,
    /// Wall-clock (`ctx.now_secs()`) at which a `CompleteDepositCashu`
    /// attempt for this quote most recently started chaining toward a
    /// signature (i.e. entered the `Minted`/`Fresh` resume branch — see
    /// `deposit.rs`'s `DepositResume`), `None` once no attempt is currently
    /// in flight. This is the concurrency guard: two attempts for the SAME
    /// `quote_id` racing each other must never both read `minted_proofs` and
    /// each launch their own encrypt/sign chain over it — that would sign
    /// TWO differently-id'd token events for one real deposit and
    /// double-fold the ledger (which has no proof-identity dedup, only
    /// token-event-id dedup). A retry that finds this set AND still fresh
    /// (within `DEPOSIT_CHAIN_LEASE_SECS`) is told to wait rather than start
    /// a second chain; past the lease, the previous attempt is presumed
    /// abandoned (its actor-thread continuation errored out with no hook
    /// back to clear this — see `chain.rs`'s `report_chain_failure` — or the
    /// process partially wedged) and a new attempt is allowed to take over. Cleared
    /// (set back to `None`) the moment signing actually succeeds
    /// (`signed_token` becomes `Some`) or a synchronous pre-chain failure
    /// returns (mint HTTP errors) — see `clear_chain_lease`.
    pub(in crate::backend::cashu) chain_started_at: Option<u64>,
    /// Set once the kind:7375 token event for `minted_proofs` has actually
    /// been SIGNED (see `deposit.rs`'s `dispatch_token_event`'s `on_signed`
    /// closure) — this is the fix for #2923's "compounding money-safety
    /// issue": signing can succeed while the publish right after it fails
    /// (no relay resolves, a relay round-trip errors, ...), and until this
    /// field existed `pending_deposits[quote_id]` was removed at sign time
    /// regardless, so a retry after a publish failure got `UNKNOWN_QUOTE`
    /// with no way back to the already-real proofs.
    ///
    /// A retry that finds this set must NEVER re-run the encrypt/sign chain
    /// — that would sign a SECOND, differently-id'd token event over the
    /// SAME proofs, and `WalletLedger` folds `TokenAdded` per token-event id
    /// with no proof-identity dedup, so a second sign would double-count the
    /// balance. It must only re-publish this EXACT cached event, which is
    /// safe to repeat: kind:7375 is a NIP-01 "regular" event (id 7375, below
    /// the 10000 replaceable-range floor), so relays dedupe repeated
    /// publishes of the same id rather than treating them as replacements.
    ///
    /// Retired once the deposit's kind:7375 is re-observed from a relay: PR-2
    /// of #2910 added the settle rule (`events.rs`'s
    /// `settle_deposit_on_ingested_token`) — the account's own token event
    /// coming back through the #2965 self-authored ingest path IS the
    /// publish-ACK this backend previously lacked, so on that re-observation the
    /// operation transitions to `Settled` and this entry is dropped. That
    /// closes the former "`pending_deposits` grows by one retained entry per
    /// completed deposit for the life of the process" tradeoff. Until that
    /// re-observation the entry is still kept (a retry before the token lands
    /// republishes this exact cached event rather than re-minting/re-signing).
    pub(in crate::backend::cashu) signed_token: Option<SignedEvent>,
}
