//! Money-safety saga: the pre-effect, durable, at-most-once state machine.
//!
//! `WalletOperationJournal` records that an operation is about to consume
//! specific inputs *before* any value-moving mint HTTP request goes out. That
//! pre-record is the at-most-once mechanism: on restart the wallet can check
//! mint/proof state and reconcile instead of risking a double-spend. This is
//! deliberately the only durable-pre-effect concern in `nmp-wallet` — derived
//! balance and the causal trail are separate schemas (see `fact` and
//! `ledger`), fed by [`WalletSagaEvent`] as a producer, never merged with the
//! saga's own state.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct WalletOperationId(String);

impl WalletOperationId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WalletOperationKind {
    SelectBackend,
    PayBolt11,
    CreateCashuWallet,
    PublishNutzapInfo,
    SendNutzap,
    RedeemNutzap,
    DepositCashu,
    MeltCashu,
    /// #2997 — `nmp.wallet.cashu.set_mints`: replaces the wallet's
    /// accepted-mint list, carrying the existing Cashu P2PK privkey forward
    /// unchanged (never rotates it, unlike `CreateCashuWallet`).
    SetCashuMints,
}

impl WalletOperationKind {
    #[must_use]
    pub const fn requires_consumed_inputs_before_mint_request(self) -> bool {
        matches!(
            self,
            Self::SendNutzap | Self::RedeemNutzap | Self::MeltCashu
        )
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WalletOperationState {
    Draft,
    Prepared,
    MintPending,
    MintSettled,
    PublishPending,
    Settled,
    Unknown,
    Failed,
}

impl WalletOperationState {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Settled | Self::Failed)
    }

    /// Note the absence of `(Self::Unknown, Self::MintPending)`: reconciling a
    /// crashed operation can only move it toward publish/settle/fail, never
    /// back into a fresh mint request. That omission is the no-double-spend
    /// guarantee — reconciliation replays the already-consumed-input record,
    /// it never re-mints.
    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Draft, Self::Prepared | Self::Failed)
                | (
                    Self::Prepared,
                    Self::MintPending | Self::PublishPending | Self::Failed
                )
                | (
                    Self::MintPending,
                    Self::MintSettled | Self::Unknown | Self::Failed
                )
                | (
                    Self::MintSettled,
                    Self::PublishPending | Self::Unknown | Self::Failed
                )
                | (
                    Self::PublishPending,
                    Self::Settled | Self::Unknown | Self::Failed
                )
                | (
                    Self::Unknown,
                    Self::MintSettled | Self::PublishPending | Self::Settled | Self::Failed
                )
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WalletConsumedInput {
    pub event_id: String,
    pub mint: String,
    pub unit: String,
    pub amount: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WalletOperation {
    pub id: WalletOperationId,
    pub kind: WalletOperationKind,
    pub state: WalletOperationState,
    pub correlation_id: Option<String>,
    pub consumed_inputs: Vec<WalletConsumedInput>,
    /// The operation's own logical amount, recorded once by the command that
    /// creates it (#2966) — distinct from `consumed_inputs`, which name
    /// proofs actually spent in wallet-internal denominations, not what the
    /// operation was *for*. Today only `SendNutzap` populates this (the
    /// amount the sender intended to deliver, known up front, before proof
    /// selection ever runs) — `consumed_inputs.last()` names the last
    /// selected proof's own face value, which is never the same number and
    /// is unset entirely for a send that fails before proof selection.
    pub recorded_amount: Option<u64>,
    /// The counterparty pubkey for a receive, set once `RedeemNutzapCommand`
    /// resolves the kind:9321 event (#2966) — a nutzap feed's "from
    /// <pubkey>" needs this and nothing upstream carried it into the journal
    /// before now. `None` for kinds with no external counterparty
    /// (`DepositCashu`, `SendNutzap`: the account itself is the sender).
    pub recorded_sender: Option<String>,
    /// When this operation was recorded, in unix seconds (#2966) — a nutzap
    /// feed's "at <time>" needs a timestamp on every history/receive row,
    /// not just a settled/failed state. Set once at `begin_operation` from
    /// the caller's already-available `ctx.now_secs`, never re-derived
    /// later, so it reads as "when this wallet started the operation"
    /// consistently across every kind.
    pub recorded_at: Option<u64>,
}

impl WalletOperation {
    #[must_use]
    pub fn new(
        id: WalletOperationId,
        kind: WalletOperationKind,
        state: WalletOperationState,
    ) -> Self {
        Self {
            id,
            kind,
            state,
            correlation_id: None,
            consumed_inputs: Vec::new(),
            recorded_amount: None,
            recorded_sender: None,
            recorded_at: None,
        }
    }

    #[must_use]
    pub fn with_correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    pub fn record_consumed_input(&mut self, input: WalletConsumedInput) {
        self.consumed_inputs.push(input);
    }

    pub fn transition(&mut self, next: WalletOperationState) -> Result<(), WalletJournalError> {
        validate_transition(self, next)?;
        self.state = next;
        Ok(())
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum WalletJournalError {
    DuplicateOperation(String),
    MissingOperation(String),
    InvalidTransition {
        from: WalletOperationState,
        to: WalletOperationState,
    },
    MissingConsumedInputs {
        operation_id: String,
        kind: WalletOperationKind,
    },
}

/// Emitted by [`WalletOperationJournal::transition`] on every successful
/// state change. This is the saga's *only* outward-facing product: the trail
/// (`crate::journal::fact::WalletFact::SagaTransition`) converts it into a
/// fact via `From<WalletSagaEvent>`, but the saga itself never depends on the
/// fact/trail types — the wiring is one-directional, producer to consumer.
/// `#[must_use]` so a call site that does `journal.transition(..)?.unwrap();`
/// and drops the result gets a compiler nudge toward feeding it into
/// `WalletFact::from` — dropping it silently means that transition never
/// reaches the causal trail.
#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use]
pub struct WalletSagaEvent {
    pub op: WalletOperationId,
    pub from: WalletOperationState,
    pub to: WalletOperationState,
}

#[derive(Clone, Debug, Default)]
pub struct WalletOperationJournal {
    operations: Vec<WalletOperation>,
}

impl WalletOperationJournal {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, operation: WalletOperation) -> Result<(), WalletJournalError> {
        if self
            .operations
            .iter()
            .any(|existing| existing.id == operation.id)
        {
            return Err(WalletJournalError::DuplicateOperation(
                operation.id.as_str().to_string(),
            ));
        }
        self.operations.push(operation);
        Ok(())
    }

    pub fn record_consumed_input(
        &mut self,
        operation_id: &WalletOperationId,
        input: WalletConsumedInput,
    ) -> Result<(), WalletJournalError> {
        let operation = self.operation_mut(operation_id)?;
        operation.record_consumed_input(input);
        Ok(())
    }

    /// Record `SendNutzap`'s intended send amount (#2966) — see
    /// [`WalletOperation::recorded_amount`]'s doc comment for why this,
    /// rather than `consumed_inputs`, is the correct source for a send
    /// history row's display amount.
    pub fn record_amount(
        &mut self,
        operation_id: &WalletOperationId,
        amount: u64,
    ) -> Result<(), WalletJournalError> {
        let operation = self.operation_mut(operation_id)?;
        operation.recorded_amount = Some(amount);
        Ok(())
    }

    /// Record a `RedeemNutzap`'s sender pubkey (#2966) once the kind:9321
    /// event resolves — see [`WalletOperation::recorded_sender`]'s doc
    /// comment.
    pub fn record_sender(
        &mut self,
        operation_id: &WalletOperationId,
        sender: impl Into<String>,
    ) -> Result<(), WalletJournalError> {
        let operation = self.operation_mut(operation_id)?;
        operation.recorded_sender = Some(sender.into());
        Ok(())
    }

    /// Transition `operation_id` to `next`, returning the [`WalletSagaEvent`]
    /// the trail should fold in as a `SagaTransition` fact. Every transition
    /// accepted by [`WalletOperationState::can_transition_to`] is a real
    /// state change (there is no no-op transition in the allowed set), so
    /// this deliberately returns a bare event rather than `Option` — an
    /// `Option` a caller could `.unwrap_or_default()` away would hide the
    /// one call site that must route this into the trail
    /// (`WalletFact::from`); the `#[must_use]` on [`WalletSagaEvent`] itself
    /// only warns when the value is a bare, unwrapped result.
    pub fn transition(
        &mut self,
        operation_id: &WalletOperationId,
        next: WalletOperationState,
    ) -> Result<WalletSagaEvent, WalletJournalError> {
        let operation = self.operation_mut(operation_id)?;
        let from = operation.state;
        operation.transition(next)?;
        Ok(WalletSagaEvent {
            op: operation_id.clone(),
            from,
            to: next,
        })
    }

    #[must_use]
    pub fn pending_operations(&self) -> Vec<WalletOperation> {
        self.operations
            .iter()
            .filter(|operation| !operation.state.is_terminal())
            .cloned()
            .collect()
    }

    /// The terminal (`Settled`/`Failed`) counterpart to
    /// [`Self::pending_operations`] — every operation whose outcome is
    /// final, in insertion order. An operation disappears from
    /// `pending_operations` the moment it reaches a terminal state; from then
    /// on this is the only journal-level view that still sees it.
    /// `CashuWalletBackend::snapshot()` folds these into
    /// `WalletProjection`'s `recent_history`/`receive_rows` (#2949) — without
    /// this, a settled or rejected operation was invisible to the projection
    /// forever.
    #[must_use]
    pub fn terminal_operations(&self) -> Vec<WalletOperation> {
        self.operations
            .iter()
            .filter(|operation| operation.state.is_terminal())
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn get(&self, operation_id: &WalletOperationId) -> Option<&WalletOperation> {
        self.operations
            .iter()
            .find(|operation| &operation.id == operation_id)
    }

    fn operation_mut(
        &mut self,
        operation_id: &WalletOperationId,
    ) -> Result<&mut WalletOperation, WalletJournalError> {
        self.operations
            .iter_mut()
            .find(|operation| &operation.id == operation_id)
            .ok_or_else(|| WalletJournalError::MissingOperation(operation_id.as_str().to_string()))
    }
}

fn validate_transition(
    operation: &WalletOperation,
    next: WalletOperationState,
) -> Result<(), WalletJournalError> {
    if !operation.state.can_transition_to(next) {
        return Err(WalletJournalError::InvalidTransition {
            from: operation.state,
            to: next,
        });
    }
    if next == WalletOperationState::MintPending
        && operation
            .kind
            .requires_consumed_inputs_before_mint_request()
        && operation.consumed_inputs.is_empty()
    {
        return Err(WalletJournalError::MissingConsumedInputs {
            operation_id: operation.id.as_str().to_string(),
            kind: operation.kind,
        });
    }
    Ok(())
}

#[cfg(test)]
#[path = "saga_tests.rs"]
mod tests;
