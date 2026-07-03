//! End-to-end acceptance tests wiring the saga, fact stream, and ledger
//! together — the scenarios epic #2864 Phase 1 calls out explicitly:
//! reconciling a crashed operation without double-spending, and restarting
//! into an honest `StateRebuilt` genesis fact rather than replaying a ring.

use nmp_wallet::{
    HistoryFactSeed, MintUrl, ProofAtom, ProofRef, WalletConsumedInput, WalletEventId, WalletFact,
    WalletLedger, WalletOperation, WalletOperationId, WalletOperationJournal, WalletOperationKind,
    WalletOperationState, WalletUnit,
};

/// Crash-after-`MintSettled`: the mint already spent the consumed inputs
/// (that already happened, durably, before this test starts) but the
/// process died before the resulting NIP-60 events were confirmed
/// published. Reconciliation from `Unknown` must republish (or confirm
/// settlement) without ever re-entering `MintPending` — re-minting would be
/// a double-spend, since the mint-side value movement already happened.
#[test]
fn crash_after_mint_settled_reconciles_without_reentering_mint_pending() {
    let mut journal = WalletOperationJournal::new();
    let op_id = WalletOperationId::new("op-crash");
    let mut op = WalletOperation::new(
        op_id.clone(),
        WalletOperationKind::SendNutzap,
        WalletOperationState::Draft,
    );
    op.record_consumed_input(WalletConsumedInput {
        event_id: "token-1".to_string(),
        mint: "https://mint.example".to_string(),
        unit: "sat".to_string(),
        amount: 21,
    });
    journal.insert(op).unwrap();

    // A real caller routes every saga transition into the trail — that is
    // the producer/consumer wiring the whole crate is built around, and
    // `WalletSagaEvent` is `#[must_use]` specifically so a call site that
    // discards it (rather than feeding it to `WalletFact::from`) gets a
    // compiler warning instead of a silently incomplete trail.
    let mut ledger = WalletLedger::new(16);
    let mut record = |journal: &mut WalletOperationJournal, next: WalletOperationState| {
        let event = journal.transition(&op_id, next).unwrap();
        ledger.apply(WalletFact::from(event));
    };

    // Reach MintSettled: the mint already settled the spend before the crash.
    record(&mut journal, WalletOperationState::Prepared);
    record(&mut journal, WalletOperationState::MintPending);
    record(&mut journal, WalletOperationState::MintSettled);

    // The process dies before publish confirmation. On restart the operation
    // is loaded back as `Unknown` (transport/process interrupted after the
    // external side effect) rather than continuing as `MintSettled`.
    record(&mut journal, WalletOperationState::Unknown);

    // The state machine itself forbids re-entering MintPending from Unknown —
    // this is the no-double-spend guarantee, structural rather than a runtime
    // check reconciliation could accidentally skip.
    assert!(!WalletOperationState::Unknown.can_transition_to(WalletOperationState::MintPending));

    // Reconciliation confirms the NIP-60 events were never published and
    // republishes them: Unknown -> PublishPending -> Settled. No second mint
    // request occurs anywhere in this path.
    record(&mut journal, WalletOperationState::PublishPending);
    record(&mut journal, WalletOperationState::Settled);

    // Every transition became a fact in the trail, so the causal history
    // shows the full path — including the crash — that ended in Settled.
    assert_eq!(ledger.ring().len(), 6);
    assert!(journal.get(&op_id).unwrap().state.is_terminal());
}

/// Restart-reconcile: wallet state after restart comes from folding durable
/// events (kind:7375 token state), entering the trail as one `StateRebuilt`
/// genesis fact — never from replaying whatever happened to still be in a
/// previous session's bounded delta ring (which does not survive restart at
/// all: a fresh `WalletLedger` is constructed).
#[test]
fn restart_reconcile_rebuilds_from_durable_events_not_a_prior_ring() {
    let mint = MintUrl::new("https://mint.example");
    let unit = WalletUnit::new("sat");

    let ledger = WalletLedger::rebuild_from(
        16,
        [
            HistoryFactSeed::TokenLive {
                token_event: WalletEventId::new("token-1"),
                mint: mint.clone(),
                unit: unit.clone(),
                proofs: vec![ProofAtom {
                    proof: ProofRef::new("proof-1"),
                    amount_msat: 1_000,
                }],
            },
            HistoryFactSeed::TokenLive {
                token_event: WalletEventId::new("token-2"),
                mint: mint.clone(),
                unit: unit.clone(),
                proofs: vec![ProofAtom {
                    proof: ProofRef::new("proof-2"),
                    amount_msat: 500,
                }],
            },
        ],
    );

    // Exactly one genesis fact for the whole restart, not one per token.
    assert_eq!(ledger.ring().len(), 1);
    assert!(matches!(
        ledger.ring().iter().next().unwrap().fact.as_ref(),
        WalletFact::StateRebuilt { .. }
    ));
    assert_eq!(ledger.state().balance(&mint, &unit), 1_500);

    // Both restored tokens honestly point back at the same genesis fact.
    for token in ["token-1", "token-2"] {
        let cause = ledger
            .causes()
            .last_event_cause(&WalletEventId::new(token))
            .expect("restored token has a cause");
        assert!(matches!(cause, WalletFact::StateRebuilt { .. }));
    }
}
