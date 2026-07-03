//! Shared in-memory state for [`super::CashuWalletBackend`].
//!
//! Held behind `Arc<Mutex<..>>` and cloned into every [`crate::backend::cashu`]
//! `ProtocolCommand` and its spawned worker thread — the same "runtime is the
//! sole writer, `snapshot()` only reads" shape `NwcWalletBackend` already
//! established for its `WalletStatusSlot` (D4). Mint HTTP round-trips happen
//! off the actor thread (D8); their workers write results back here directly
//! rather than bouncing through a second actor round-trip.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use crate::journal::{
    WalletFact, WalletJournalError, WalletLedger, WalletOperation, WalletOperationId,
    WalletOperationJournal, WalletOperationKind, WalletOperationState,
};

/// Bounded delta-ring capacity for this backend's [`WalletLedger`] — matches
/// the order of magnitude `WalletProjection`'s own row cap
/// (`MAX_WALLET_PROJECTION_ROWS`) uses; the ring is a diagnostic surface, not
/// the rebuild authority (see `journal::ledger` module docs).
const DELTA_RING_CAPACITY: usize = 256;

/// A deposit whose quote has been requested but not yet completed —
/// [`crate::backend::WalletIntent::CompleteDeposit`] looks operations up by
/// `quote_id` (the caller's only handle on a pending deposit; see the action
/// result surfacing in `deposit.rs`).
#[derive(Clone)]
pub(super) struct PendingDeposit {
    pub(super) operation_id: WalletOperationId,
    pub(super) mint: String,
    pub(super) amount_sats: u64,
}

pub(super) struct CashuWalletState {
    /// Whether `CreateCashuWallet` has completed (kind:17375 signed and
    /// handed to the publish pipeline). Drives `snapshot()`'s readiness.
    pub(super) created: bool,
    /// Mints this wallet accepts — the allow-list `DepositQuote` validates
    /// against ("unsupported mint" is "not in this list", never a fixed
    /// global whitelist; MVP defaults the shell's suggested mint to
    /// `https://testnut.cashu.space`, but any mint the wallet was created
    /// with is accepted).
    pub(super) mints: Vec<String>,
    /// The wallet's Cashu P2PK pubkey (NIP-61 receiving key), once created.
    pub(super) cashu_pubkey_hex: Option<String>,
    pub(super) journal: WalletOperationJournal,
    pub(super) ledger: WalletLedger,
    /// quote_id -> pending deposit, keyed by the id `CompleteDeposit` carries.
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
            journal: WalletOperationJournal::new(),
            ledger: WalletLedger::new(DELTA_RING_CAPACITY),
            pending_deposits: BTreeMap::new(),
        }
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
