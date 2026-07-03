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
mod tests {
    use super::*;

    fn input() -> WalletConsumedInput {
        WalletConsumedInput {
            event_id: "event-1".to_string(),
            mint: "https://mint.example".to_string(),
            unit: "sat".to_string(),
            amount: 21,
        }
    }

    #[test]
    fn value_moving_mint_request_requires_recorded_inputs() {
        let mut operation = WalletOperation::new(
            WalletOperationId::new("op-send"),
            WalletOperationKind::SendNutzap,
            WalletOperationState::Prepared,
        );

        assert_eq!(
            operation.transition(WalletOperationState::MintPending),
            Err(WalletJournalError::MissingConsumedInputs {
                operation_id: "op-send".to_string(),
                kind: WalletOperationKind::SendNutzap,
            })
        );

        operation.record_consumed_input(input());
        assert!(operation
            .transition(WalletOperationState::MintPending)
            .is_ok());
    }

    #[test]
    fn terminal_operations_do_not_transition_again() {
        let mut operation = WalletOperation::new(
            WalletOperationId::new("op-settled"),
            WalletOperationKind::PayBolt11,
            WalletOperationState::Settled,
        );

        assert_eq!(
            operation.transition(WalletOperationState::Failed),
            Err(WalletJournalError::InvalidTransition {
                from: WalletOperationState::Settled,
                to: WalletOperationState::Failed,
            })
        );
    }

    #[test]
    fn journal_lists_only_pending_operations() {
        let mut journal = WalletOperationJournal::new();
        journal
            .insert(WalletOperation::new(
                WalletOperationId::new("pending"),
                WalletOperationKind::DepositCashu,
                WalletOperationState::MintPending,
            ))
            .unwrap();
        journal
            .insert(WalletOperation::new(
                WalletOperationId::new("done"),
                WalletOperationKind::DepositCashu,
                WalletOperationState::Settled,
            ))
            .unwrap();

        let pending = journal.pending_operations();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id.as_str(), "pending");
    }

    #[test]
    fn journal_transition_emits_a_saga_event_for_the_trail() {
        let mut journal = WalletOperationJournal::new();
        journal
            .insert(WalletOperation::new(
                WalletOperationId::new("op-1"),
                WalletOperationKind::PayBolt11,
                WalletOperationState::Draft,
            ))
            .unwrap();

        let event = journal
            .transition(
                &WalletOperationId::new("op-1"),
                WalletOperationState::Prepared,
            )
            .unwrap();

        assert_eq!(event.op.as_str(), "op-1");
        assert_eq!(event.from, WalletOperationState::Draft);
        assert_eq!(event.to, WalletOperationState::Prepared);
    }

    #[test]
    fn reconciling_an_unknown_operation_can_never_reach_mint_pending_again() {
        // This is the no-double-spend guarantee: restart reconciliation may
        // only move an `Unknown` operation toward publish/settle/fail, never
        // back into a fresh mint request.
        assert!(!WalletOperationState::Unknown.can_transition_to(WalletOperationState::MintPending));
    }
}
