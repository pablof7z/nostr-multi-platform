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
    pub(super) minted_proofs: Option<Vec<Proof>>,
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
        }
    }

    /// Select proofs from `mint` summing to at least `amount_sats`, greedily
    /// (no change-minimizing search — matches
    /// `nip60_wallet::nutzap_send::select_proofs`'s own greedy shape).
    /// Returns `None` when this mint's held proofs don't cover the amount;
    /// callers must fail closed on `None` rather than partially spend.
    /// Read-only — does not remove the proofs; see [`Self::remove_proofs`].
    pub(super) fn select_proofs(
        &self,
        mint: &str,
        amount_sats: u64,
    ) -> Option<(Vec<StoredProof>, u64)> {
        let mut selected = Vec::new();
        let mut total = 0u64;
        for stored in self.proofs.iter().filter(|p| p.mint == mint) {
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
    /// attached to the same `token_event`.
    pub(super) fn add_proofs(&mut self, token_event: Option<String>, mint: String, proofs: Vec<Proof>) {
        self.proofs.extend(proofs.into_iter().map(|proof| StoredProof {
            token_event: token_event.clone(),
            mint: mint.clone(),
            proof,
        }));
    }

    /// Insert a fresh operation and drive it Draft -> Prepared, folding the
    /// resulting `WalletSagaEvent` into the ledger as `SagaTransition` facts
    /// (the saga is a *producer* into the trail — see `journal` module docs).
    /// This durably records "an operation for this intent now exists" before
    /// any HTTP or port round-trip starts.
    pub(super) fn begin_operation(
        &mut self,
        id: WalletOperationId,
        kind: WalletOperationKind,
    ) -> Result<(), WalletJournalError> {
        self.journal.insert(WalletOperation::new(
            id.clone(),
            kind,
            WalletOperationState::Draft,
        ))?;
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
