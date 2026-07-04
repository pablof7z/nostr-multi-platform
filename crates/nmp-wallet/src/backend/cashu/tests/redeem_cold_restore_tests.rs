//! Cold-restart redeem recovery (PR-3 of #2910/#2931): drop the whole
//! in-memory backend, reconstruct a fresh one over the same on-disk WAL, and
//! prove the redeem resumes/reconciles correctly from each crash point —
//!
//! - swapped, not published: `finish_redeem` is re-driven from the persisted
//!   fresh proofs (publishes kind:7375 then kind:7376).
//! - `Unknown`, inputs unspent: the swap never committed → the saga row is
//!   deleted from BOTH the durable WAL and the in-memory journal, so a
//!   re-observed kind:9321 can `begin_operation` cleanly — **the #2931 fix**,
//!   proven end-to-end below.
//! - `Unknown`, an input spent: the swap committed and the fresh proofs were
//!   lost — the redeem is left `Unknown` and surfaces in `pending_operations`
//!   as needing attention rather than being silently dropped.

use std::sync::Arc;
use std::time::Duration;

use super::*;
use nmp_core::actor::SignCommand;
use nmp_nip60::cashu::types::Proof;
use nmp_nip60::nutzap::{NutZapProof, ReceivedNutZap};

use crate::journal::{
    FsWalletWalStore, WalletConsumedInput, WalletEventId, WalletOperationId, WalletOperationKind,
    WalletOperationState, WalletWalStore,
};

/// A valid 64-hex kind:9321 event id every fixture here keys on.
fn nutzap_event_id() -> String {
    "d".repeat(64)
}

fn fs_store(dir: &std::path::Path) -> Arc<dyn WalletWalStore> {
    Arc::new(FsWalletWalStore::new(dir)) as Arc<dyn WalletWalStore>
}

fn y_hex(secret: &str) -> String {
    hex::encode(
        nmp_nip60::cashu::crypto::hash_to_curve(secret.as_bytes())
            .expect("hash_to_curve")
            .serialize(),
    )
}

/// A received nutzap whose input proof carries `secret` (so a mock mint can
/// echo its `Y`), authored by a throwaway sender, on `mint`.
fn received_nutzap(mint: &str, secret: &str, amount: u64) -> ReceivedNutZap {
    let sender = nostr::Keys::generate();
    ReceivedNutZap {
        event_id: nostr::EventId::from_hex(&nutzap_event_id()).unwrap(),
        sender_pubkey: sender.public_key(),
        proofs: vec![NutZapProof {
            amount,
            id: "keyset-1".to_string(),
            secret: secret.to_string(),
            c: "02aa".to_string(),
            dleq: None,
        }],
        mint_url: mint.to_string(),
        amount_sats: amount,
        comment: String::new(),
        zapped_event_id: None,
    }
}

/// Write a redeem at a given crash stage into the durable WAL under `account`,
/// then drop the backend. `fresh_proofs`/`make_unknown` select the crash point.
fn seed_redeem(
    dir: &std::path::Path,
    account: &str,
    nutzap: &ReceivedNutZap,
    fresh_proofs: Option<Vec<Proof>>,
    make_unknown: bool,
) -> WalletOperationId {
    let backend = CashuWalletBackend::with_wal_store(Some(fs_store(dir)));
    let _ = backend.restore_from_wal(account);
    let mut state = state::lock_state(&backend.state);
    let op = WalletOperationId::new(format!("redeem-{}", nutzap_event_id()));
    let nwe = WalletEventId::new(nutzap_event_id());
    state
        .begin_operation(op.clone(), WalletOperationKind::RedeemNutzap)
        .unwrap();
    // RedeemNutzap requires a consumed input before the MintPending transition.
    state
        .record_consumed_input(
            &op,
            WalletConsumedInput {
                event_id: nutzap_event_id(),
                mint: nutzap.mint_url.clone(),
                unit: "sat".to_string(),
                amount: nutzap.amount_sats,
            },
        )
        .unwrap();
    state
        .transition(&op, WalletOperationState::MintPending)
        .unwrap();
    if make_unknown {
        state
            .transition(&op, WalletOperationState::Unknown)
            .unwrap();
    }
    wal_redeem::persist_redeem_payload(
        &state,
        &op,
        nutzap,
        &nwe,
        &["wss://my-relay.example".to_string()],
        fresh_proofs,
    );
    op
}

fn run_resume_first_command(cmd: ActorCommand, relays: Vec<String>) -> ActorCommand {
    let protocol = match cmd {
        ActorCommand::Protocol(p) => p,
        other => panic!("expected a Protocol(ResumeRedeemCommand), got {other:?}"),
    };
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_100);
    let recipients = FixedRecipientLookup(relays);
    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    {
        let mut ctx = ctx_with_sender(
            &send,
            nmp_core::CommandSender::new(worker_tx),
            &clock,
            &recipients,
        );
        protocol.run(&mut ctx).expect("resume run returns Ok");
    }
    recv_command(&worker_rx)
}

fn run_reconcile_and_wait(cmd: ActorCommand) {
    let protocol = match cmd {
        ActorCommand::Protocol(p) => p,
        other => panic!("expected a Protocol(ResumeRedeemCommand), got {other:?}"),
    };
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_100);
    let recipients = FixedRecipientLookup(Vec::new());
    let (worker_tx, _worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    {
        let mut ctx = ctx_with_sender(
            &send,
            nmp_core::CommandSender::new(worker_tx),
            &clock,
            &recipients,
        );
        protocol.run(&mut ctx).expect("reconcile run returns Ok");
    }
    std::thread::sleep(Duration::from_millis(400));
}

/// swapped-not-published: the persisted fresh proofs re-drive `finish_redeem`.
/// The first worker command is the kind:7375 NIP-44 encrypt step (never a mint
/// HTTP call) — proving the re-drive resumed straight into `finish_redeem`.
#[test]
fn swapped_not_published_restore_re_drives_finish_redeem() {
    let dir = tempfile::tempdir().unwrap();
    let account = "aa".repeat(32);
    let nutzap = received_nutzap(MINT, "sec-a", 21);
    seed_redeem(
        dir.path(),
        &account,
        &nutzap,
        Some(vec![synthetic_proof(21, "02fresh")]),
        false,
    );

    let backend = CashuWalletBackend::with_wal_store(Some(fs_store(dir.path())));
    let mut resumes = backend.restore_from_wal(&account);
    assert_eq!(resumes.len(), 1, "a swapped redeem must be re-driven");

    match run_resume_first_command(resumes.remove(0), vec!["wss://my-relay.example".to_string()]) {
        ActorCommand::Sign(SignCommand::Nip44EncryptForAccount { .. }) => {}
        other => panic!(
            "expected the re-drive to resume into finish_redeem's kind:7375 encrypt step, got {other:?}"
        ),
    }
}

/// The #2931 fix, end-to-end: an operation stuck in `Unknown`, restart,
/// check-state says the inputs are unspent (the swap never committed), the row
/// is deleted from both the durable WAL and the in-memory journal, and a fresh
/// `begin_operation` for the SAME redeem id succeeds afterward — proving a
/// naturally re-observed kind:9321 is no longer blocked by `DuplicateOperation`.
#[test]
fn unknown_unspent_restore_deletes_row_and_unblocks_redispatch() {
    let dir = tempfile::tempdir().unwrap();
    let account = "bb".repeat(32);
    let body = serde_json::json!({
        "states": [{"Y": y_hex("sec-a"), "state": "UNSPENT"}]
    })
    .to_string();
    let mock = spawn_mock_mint(vec![(200, body)]);
    let nutzap = received_nutzap(&mock, "sec-a", 21);
    let op = seed_redeem(dir.path(), &account, &nutzap, None, true);

    let store = fs_store(dir.path());
    let backend = CashuWalletBackend::with_wal_store(Some(Arc::clone(&store)));
    // Precondition: the stuck Unknown redeem is on disk and rehydrated live.
    let mut resumes = backend.restore_from_wal(&account);
    assert_eq!(resumes.len(), 1, "a stuck Unknown redeem must be reconciled");
    assert_eq!(store.load_operations(&account).unwrap().len(), 1);

    run_reconcile_and_wait(resumes.remove(0));

    // The row is gone from the in-memory journal AND the durable WAL.
    {
        let state = state::lock_state(&backend.state);
        assert!(
            state.journal.get(&op).is_none(),
            "the Unknown redeem is removed from the in-memory journal (not left Failed)"
        );
    }
    assert!(
        store.load_operations(&account).unwrap().is_empty(),
        "the durable saga row is deleted"
    );
    assert!(
        store.load_payload(&account, &op).unwrap().is_none(),
        "the durable resume payload is deleted"
    );

    // The load-bearing consequence: a re-observation of the same kind:9321 runs
    // begin_operation cleanly, NOT blocked by DuplicateOperation.
    let mut state = state::lock_state(&backend.state);
    state
        .begin_operation(op.clone(), WalletOperationKind::RedeemNutzap)
        .expect("re-observation must not be blocked after the Unknown row is reconciled away");
}

/// `Unknown`, an input spent: the swap committed and the fresh proofs were lost
/// pre-crash — the redeem is left `Unknown` and surfaces in
/// `pending_operations`; it is never deleted or silently dropped.
#[test]
fn unknown_spent_restore_leaves_redeem_unknown_and_surfaced() {
    let dir = tempfile::tempdir().unwrap();
    let account = "cc".repeat(32);
    let body = serde_json::json!({
        "states": [{"Y": y_hex("sec-a"), "state": "SPENT"}]
    })
    .to_string();
    let mock = spawn_mock_mint(vec![(200, body)]);
    let nutzap = received_nutzap(&mock, "sec-a", 21);
    let op = seed_redeem(dir.path(), &account, &nutzap, None, true);

    let store = fs_store(dir.path());
    let backend = CashuWalletBackend::with_wal_store(Some(Arc::clone(&store)));
    let mut resumes = backend.restore_from_wal(&account);
    assert_eq!(resumes.len(), 1);

    run_reconcile_and_wait(resumes.remove(0));

    let state = state::lock_state(&backend.state);
    assert_eq!(
        state.journal.get(&op).map(|o| o.state),
        Some(WalletOperationState::Unknown),
        "a spent input leaves the redeem Unknown, never deleted"
    );
    assert!(
        state
            .journal
            .pending_operations()
            .iter()
            .any(|o| o.id == op),
        "the Unknown redeem stays in pending_operations, surfaced as needing attention"
    );
    // Still on disk — nothing was reconciled away.
    drop(state);
    assert_eq!(store.load_operations(&account).unwrap().len(), 1);
}
