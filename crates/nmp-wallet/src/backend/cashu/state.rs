//! Shared in-memory state for [`super::CashuWalletBackend`].
//!
//! Held behind `Arc<Mutex<..>>` and cloned into every [`crate::backend::cashu`]
//! `ProtocolCommand` and its spawned worker thread — the same "runtime is the
//! sole writer, `snapshot()` only reads" shape `NwcWalletBackend` already
//! established for its `WalletStatusSlot` (D4). Mint HTTP round-trips happen
//! off the actor thread (D8); their workers write results back here directly
//! rather than bouncing through a second actor round-trip.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Mutex, MutexGuard};

use nmp_nip60::cashu::types::Proof;
use nmp_signer_iface::SignedEvent;

use crate::journal::{
    WalletFact, WalletJournalError, WalletLedger, WalletOperation, WalletOperationId,
    WalletOperationJournal, WalletOperationKind, WalletOperationState,
};

/// The wallet's Cashu (NUT-11 P2PK) private key — NOT the Nostr identity key
/// (see `create_wallet.rs`). Held only in this in-memory state, never in a
/// `WalletFact`/journal/projection/log line: `Debug` is hand-redacted so a
/// stray `{:?}` on `CashuWalletState` (or anything holding this) can never
/// print it. Needed to sign received P2PK proofs on redeem (`sign_p2pk_proof`)
/// — cold-start recovery of this value from the kind:17375 wallet event is a
/// documented, separate deferral (see `CashuWalletBackend::on_wallet_event`'s
/// doc comment); without it in live state, `RedeemNutzap` fails closed.
pub(super) struct CashuP2pkSecret(pub(super) nostr::secp256k1::SecretKey);

impl fmt::Debug for CashuP2pkSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CashuP2pkSecret").field(&"<redacted>").finish()
    }
}

/// One real, spendable Cashu proof this wallet currently holds — secret
/// material (the proof's `secret`/`witness`), never surfaced through
/// `WalletFact`/`WalletProjection`/a log line. Held ONLY here, alongside
/// [`CashuP2pkSecret`]; the ledger's `ProofAtom` (proof-ref + amount only) is
/// the privacy-safe shadow of this same proof for balance/trail purposes —
/// the two are kept in lockstep by every caller that mutates both (never one
/// without the other).
#[derive(Clone)]
pub(super) struct StoredProof {
    /// Hex id of the kind:7375 token event this proof currently lives on
    /// (for building the NIP-09 delete + replacement token event when it is
    /// spent). `None` for a proof not yet attached to a published token
    /// event (there is no such producer today, but the field stays optional
    /// rather than a placeholder string).
    pub(super) token_event: Option<String>,
    pub(super) mint: String,
    pub(super) proof: Proof,
}

/// Bounded delta-ring capacity for this backend's [`WalletLedger`] — matches
/// the order of magnitude `WalletProjection`'s own row cap
/// (`MAX_WALLET_PROJECTION_ROWS`) uses; the ring is a diagnostic surface, not
/// the rebuild authority (see `journal::ledger` module docs).
const DELTA_RING_CAPACITY: usize = 256;

/// A deposit whose quote has been requested but not yet completed —
/// [`crate::backend::WalletIntent::CompleteDepositCashu`] looks operations up by
/// `quote_id` (the caller's only handle on a pending deposit; see the action
/// result surfacing in `deposit.rs`).
#[derive(Clone)]
pub(super) struct PendingDeposit {
    pub(super) operation_id: WalletOperationId,
    pub(super) mint: String,
    pub(super) amount_sats: u64,
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
    /// This field is in-memory only — it does NOT survive a process crash or
    /// restart. A hard crash in the window between `mint_tokens` returning
    /// `Ok` and the kind:7375 event publishing loses these proofs for real;
    /// closing that window needs a durable write-ahead record, tracked as
    /// issue #2910 (not this ticket's scope — real-sats gate, not a
    /// testnut/merge gate).
    ///
    /// Never cleared once set (mirrors `signed_token`): kept around both for
    /// sequential retries AND as the reference proof set `still_held`
    /// (`deposit.rs`) checks a cached `signed_token` against before
    /// republishing it.
    pub(super) minted_proofs: Option<Vec<Proof>>,
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
    pub(super) chain_started_at: Option<u64>,
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
    /// This entry is intentionally never cleared/removed once set — there is
    /// no publish-ACK loop back into this backend to know when it is finally
    /// safe to forget (see `dispatch_token_event`'s doc comment); a bounded
    /// `pending_deposits` map growing by one settled entry per deposit for
    /// the life of the process is the accepted tradeoff pending #2910.
    pub(super) signed_token: Option<SignedEvent>,
}

pub(super) struct CashuWalletState {
    /// Whether `CreateCashuWallet` has completed (kind:17375 signed and
    /// handed to the publish pipeline). Drives `snapshot()`'s readiness.
    pub(super) created: bool,
    /// Mints this wallet accepts — the allow-list `DepositQuoteCashu` validates
    /// against ("unsupported mint" is "not in this list", never a fixed
    /// global whitelist; MVP defaults the shell's suggested mint to
    /// `https://testnut.cashu.space`, but any mint the wallet was created
    /// with is accepted).
    pub(super) mints: Vec<String>,
    /// The wallet's Cashu P2PK pubkey (NIP-61 receiving key), once created.
    pub(super) cashu_pubkey_hex: Option<String>,
    /// The Cashu P2PK private key paired with `cashu_pubkey_hex` — see
    /// [`CashuP2pkSecret`]'s doc comment for why this lives here and not in
    /// the ledger/facts.
    pub(super) cashu_privkey: Option<CashuP2pkSecret>,
    /// Real, spendable proofs this wallet currently holds — the secret-
    /// bearing counterpart to the ledger's aggregate, ref-only balance.
    /// `SendNutzap`/`RedeemNutzap` are the only readers/writers.
    pub(super) proofs: Vec<StoredProof>,
    pub(super) journal: WalletOperationJournal,
    pub(super) ledger: WalletLedger,
    /// quote_id -> pending deposit, keyed by the id `CompleteDepositCashu` carries.
    /// Never surfaced through `WalletProjection` (bounded product shape) or a
    /// log line — quote ids are secret-adjacent (ties the wallet to a
    /// specific pending payment); they only ever cross through the
    /// `RecordActionSuccess` one-shot action result a caller must keep to
    /// complete the deposit.
    pub(super) pending_deposits: BTreeMap<String, PendingDeposit>,
    /// #2977 single-flight guard for `check_state::spawn_debounced` — the
    /// passive cold-start-replay trigger (`ingest.rs`'s
    /// `build_passive_ingest_command`) can fold fresh proofs from many
    /// kind:7375 events in a tight, unordered burst; without this, each
    /// would spawn its own full check-state pass over every held proof,
    /// hammering the same mint(s) with redundant concurrent
    /// `/v1/checkstate` calls. `true` while a pass is running.
    pub(super) check_state_in_flight: bool,
    /// Set when a trigger arrives while `check_state_in_flight` is already
    /// `true` — the in-flight pass re-runs once more before clearing
    /// `check_state_in_flight`, so proofs folded mid-pass are coalesced into
    /// the next run rather than permanently skipped.
    pub(super) check_state_rerun_needed: bool,
}

impl CashuWalletState {
    pub(super) fn new() -> Self {
        Self {
            created: false,
            mints: Vec::new(),
            cashu_pubkey_hex: None,
            cashu_privkey: None,
            proofs: Vec::new(),
            journal: WalletOperationJournal::new(),
            ledger: WalletLedger::new(DELTA_RING_CAPACITY),
            pending_deposits: BTreeMap::new(),
            check_state_in_flight: false,
            check_state_rerun_needed: false,
        }
    }

    /// Select proofs from `mint` summing to at least `amount_sats`, greedily
    /// (no change-minimizing search — matches
    /// `nip60_wallet::nutzap_send::select_proofs`'s own greedy shape).
    /// Returns `None` when this mint's held proofs don't cover the amount;
    /// callers must fail closed on `None` rather than partially spend.
    /// Read-only — does not remove the proofs; see [`Self::remove_proofs`].
    ///
    /// Compares by [`canonicalize_mint_url`], not raw string equality (#2972)
    /// — `mint` here is often a caller-resolved string (e.g. `send.rs`'s
    /// recipient-tag mint) that can differ from a stored proof's `mint`
    /// (`add_proofs` already stores the canonical form, but this side
    /// canonicalizes too rather than assuming that invariant holds forever).
    pub(super) fn select_proofs(
        &self,
        mint: &str,
        amount_sats: u64,
    ) -> Option<(Vec<StoredProof>, u64)> {
        let target = canonicalize_mint_url(mint);
        let mut selected = Vec::new();
        let mut total = 0u64;
        for stored in self.proofs.iter().filter(|p| canonicalize_mint_url(&p.mint) == target) {
            if total >= amount_sats {
                break;
            }
            selected.push(stored.clone());
            total += stored.proof.amount;
        }
        if total < amount_sats {
            return None;
        }
        Some((selected, total))
    }

    /// Remove exactly the proofs in `spent` (matched by the proof's public
    /// `C` value — unique per proof) from the held inventory. Call this only
    /// once the mint has actually consumed them (i.e. after a successful
    /// swap), never speculatively.
    pub(super) fn remove_proofs(&mut self, spent: &[StoredProof]) {
        let spent_cs: std::collections::HashSet<&str> =
            spent.iter().map(|p| p.proof.c.as_str()).collect();
        self.proofs.retain(|p| !spent_cs.contains(p.proof.c.as_str()));
    }

    /// Add freshly minted/received proofs to the held inventory, all
    /// attached to the same `token_event`. Stores [`canonicalize_mint_url`]'s
    /// output, not `mint` verbatim (#2972) — the caller-supplied string
    /// (a deposit's typed mint, a redeemed nutzap's `u`-tag mint, ...) is
    /// never guaranteed byte-identical to the string `select_proofs`/send
    /// resolution later looks the same mint up by, even though both denote
    /// the same real mint.
    pub(super) fn add_proofs(&mut self, token_event: Option<String>, mint: String, proofs: Vec<Proof>) {
        let mint = canonicalize_mint_url(&mint);
        self.proofs.extend(proofs.into_iter().map(|proof| StoredProof {
            token_event: token_event.clone(),
            mint: mint.clone(),
            proof,
        }));
    }

    /// Drop every held proof attached to `token_event` — the recovery-path
    /// (#2965) counterpart to [`Self::remove_proofs`]: a kind:7375 token
    /// event superseded by a newer one's `del` field is dead regardless of
    /// which proofs it carried, so this removes by token-event id rather
    /// than needing the superseded event's own (possibly not-yet-decrypted)
    /// proof list. Safe to call for a `token_event` this wallet never
    /// actually held (out-of-order recovery replay) — a plain no-op.
    pub(super) fn remove_proofs_for_token_event(&mut self, token_event: &str) {
        self.proofs
            .retain(|p| p.token_event.as_deref() != Some(token_event));
    }

    /// Insert a fresh operation and drive it Draft -> Prepared, folding the
    /// resulting `WalletSagaEvent` into the ledger as `SagaTransition` facts
    /// (the saga is a *producer* into the trail — see `journal` module docs).
    /// This durably records "an operation for this intent now exists" before
    /// any HTTP or port round-trip starts.
    ///
    /// Kept as the plain (no timestamp) form so the many existing tests that
    /// drive the journal directly don't need touching; every real dispatch
    /// site goes through [`Self::begin_operation_at`] instead, which is the
    /// one that actually populates `WalletOperation::recorded_at` (#2966).
    #[cfg(test)]
    pub(super) fn begin_operation(
        &mut self,
        id: WalletOperationId,
        kind: WalletOperationKind,
    ) -> Result<(), WalletJournalError> {
        self.begin_operation_at(id, kind, 0)
    }

    /// [`Self::begin_operation`], plus recording `now_secs` as the
    /// operation's [`WalletOperation::recorded_at`] (#2966) — every history/
    /// receive row needs a timestamp, and the caller's `ctx.now_secs` is
    /// already the wallet's one clock read for this dispatch, so this
    /// threads it through rather than taking a fresh, separately-sourced
    /// timestamp.
    pub(super) fn begin_operation_at(
        &mut self,
        id: WalletOperationId,
        kind: WalletOperationKind,
        now_secs: u64,
    ) -> Result<(), WalletJournalError> {
        let mut operation = WalletOperation::new(id.clone(), kind, WalletOperationState::Draft);
        operation.recorded_at = Some(now_secs);
        self.journal.insert(operation)?;
        self.transition(&id, WalletOperationState::Prepared)
    }

    /// The single saga-transition chokepoint every caller (backend, commands,
    /// and workers) goes through, so every journal transition is also folded
    /// into the ledger's causal trail — never call `self.journal.transition`
    /// directly from outside this type.
    pub(super) fn transition(
        &mut self,
        id: &WalletOperationId,
        next: WalletOperationState,
    ) -> Result<(), WalletJournalError> {
        let event = self.journal.transition(id, next)?;
        self.ledger.apply(WalletFact::from(event));
        Ok(())
    }
}

/// D6/D4: a poisoned mutex is recovered rather than collapsed — the
/// last-written state is still the best information available (mirrors
/// `NwcWalletBackend::current_status`'s poison handling for `WalletStatusSlot`).
pub(super) fn lock_state(state: &Mutex<CashuWalletState>) -> MutexGuard<'_, CashuWalletState> {
    match state.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Non-network, non-secret gate: an accepted mint URL must be a non-empty
/// `http(s)://` string. This is deliberately NOT a fixed whitelist — MVP
/// defaults to suggesting `https://testnut.cashu.space` at the shell layer,
/// but any well-formed mint URL is accepted here (fail-closed only rejects
/// obviously-malformed input, not "not testnut").
pub(super) fn is_well_formed_mint_url(mint: &str) -> bool {
    let trimmed = mint.trim();
    !trimmed.is_empty() && (trimmed.starts_with("https://") || trimmed.starts_with("http://"))
}

/// Normalize a Cashu mint HTTP URL for equality comparisons (#2972) — two
/// strings that name the same real mint (a trailing slash, a differently-
/// cased scheme/host) must compare equal wherever this wallet matches a
/// caller-resolved mint (a recipient's kind:10019 `u` tag, a deposit's typed
/// mint) against its own stored proofs' mint.
///
/// Deliberately narrower than `nmp_relay_url::canonicalize` (which owns a
/// relay WebSocket URL end-to-end, including its path): a Cashu mint URL's
/// PATH is semantically load-bearing (e.g. minibits serves a distinct
/// endpoint per unit at `/Bitcoin`), so only the scheme and host are
/// lowercased and only a trailing `/` is stripped — the path's case and
/// interior segments are preserved untouched. Two URLs that differ by path
/// (even just by case) name *different* mints and must never collapse to
/// the same canonical string.
///
/// Falls back to the trimmed, unmodified input when the string has no
/// `scheme://` separator — never panics, and never invents a canonical form
/// for something that isn't a well-formed URL to begin with (see
/// `is_well_formed_mint_url`, which gates malformed input earlier in the
/// pipeline).
///
/// Only the authority (host[:port]) is case-folded — the split point is the
/// first of `/`, `?`, or `#`, so a query string or fragment (unlikely for a
/// mint URL, but not this function's business to rewrite) is left completely
/// untouched rather than accidentally lowercased along with the host. And
/// only ONE trailing `/` is ever stripped, from the end of the path
/// specifically (before any `?`/`#`): `/Bitcoin//` and `/Bitcoin/` still
/// compare distinct from each other (only from `/Bitcoin`'s own
/// single-trailing-slash form) — this function corrects exactly the
/// single-trailing-slash case #2972 hit, not a general "collapse repeated
/// slashes" rule.
pub(super) fn canonicalize_mint_url(mint: &str) -> String {
    let trimmed = mint.trim();
    let Some((scheme, rest)) = trimmed.split_once("://") else {
        return trimmed.to_string();
    };
    let scheme = scheme.to_ascii_lowercase();
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = rest[..authority_end].to_ascii_lowercase();
    let mut remainder = rest[authority_end..].to_string();
    let path_end = remainder.find(['?', '#']).unwrap_or(remainder.len());
    if path_end > 0 && remainder.as_bytes()[path_end - 1] == b'/' {
        remainder.remove(path_end - 1);
    }
    format!("{scheme}://{authority}{remainder}")
}
