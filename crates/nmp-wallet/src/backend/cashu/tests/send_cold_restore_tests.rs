//! Cold-restart send recovery (PR-3 of #2910/#2960): drop the whole in-memory
//! backend, reconstruct a fresh one over the same on-disk WAL, and prove the
//! send resumes correctly from each crash point —
//!
//! - swapped, not signed: `finish_send` is re-driven from the persisted
//!   post-swap proofs (rebuilds/signs the kind:9321) — the #2960 fix.
//! - reserved, swap never committed (inputs unspent at the mint): the send is
//!   failed (terminal → its WAL row is deleted).
//! - reserved, an input is spent (the swap may have committed and the outputs
//!   were lost): the send is left `Unknown` so it surfaces in
//!   `pending_operations` as needing attention.

use std::sync::Arc;
use std::time::Duration;

use super::*;
use nmp_core::actor::SignCommand;
use nmp_nip60::cashu::types::Proof;
use nmp_nip60::kinds::KIND_NIP61_NUTZAP;

use crate::journal::{
    FsWalletWalStore, WalletConsumedInput, WalletOperationId, WalletOperationKind,
    WalletOperationState, WalletWalStore,
};

const RECIPIENT: &str = "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";
const RECIPIENT_CASHU: &str = "02bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn fs_store(dir: &std::path::Path) -> Arc<dyn WalletWalStore> {
    Arc::new(FsWalletWalStore::new(dir)) as Arc<dyn WalletWalStore>
}

/// The `Y = hash_to_curve(secret)` hex the mint check-state derives — a mock
/// mint must echo exactly this back (same helper as `check_state_tests`).
fn y_hex(secret: &str) -> String {
    hex::encode(
        nmp_nip60::cashu::crypto::hash_to_curve(secret.as_bytes())
            .expect("hash_to_curve")
            .serialize(),
    )
}

fn proof(amount: u64, c: &str, secret: &str) -> Proof {
    Proof {
        amount,
        id: "keyset-1".to_string(),
        secret: secret.to_string(),
        c: c.to_string(),
        dleq: None,
        witness: None,
    }
}

fn stored(mint: &str, p: Proof) -> state::StoredProof {
    state::StoredProof {
        token_event: Some("token-event-1".to_string()),
        mint: mint.to_string(),
        proof: p,
    }
}

/// Write a send at a given crash stage into the durable WAL under `account`,
/// then drop the backend — the "process that crashed" whose only surviving
/// trace is the on-disk WAL. `swapped` selects the crash point.
fn seed_send(
    dir: &std::path::Path,
    account: &str,
    op_id: &str,
    mint: &str,
    selected: &[state::StoredProof],
    swapped: Option<wal_payload::SwappedSend>,
) {
    let backend = CashuWalletBackend::with_wal_store(Some(fs_store(dir)));
    // Arms write-through by setting `wal_account`.
    let _ = backend.restore_from_wal(account);
    let mut state = state::lock_state(&backend.state);
    let op = WalletOperationId::new(op_id);
    state
        .begin_operation(op.clone(), WalletOperationKind::SendNutzap)
        .unwrap();
    // SendNutzap requires consumed inputs before the MintPending transition.
    for s in selected {
        state
            .record_consumed_input(
                &op,
                WalletConsumedInput {
                    event_id: s.token_event.clone().unwrap_or_default(),
                    mint: mint.to_string(),
                    unit: "sat".to_string(),
                    amount: s.proof.amount,
                },
            )
            .unwrap();
    }
    state
        .transition(&op, WalletOperationState::MintPending)
        .unwrap();
    wal_send::persist_send_payload(
        &state,
        &op,
        mint,
        RECIPIENT,
        RECIPIENT_CASHU,
        None,
        &["wss://recipient-relay.example".to_string()],
        selected,
        swapped,
    );
}

/// Run a returned resume command and return the FIRST command its worker thread
/// emits — enough to prove which chain tail it resumed into (mirrors
/// `deposit_cold_restore_tests`'s helper).
fn run_resume_first_command(cmd: ActorCommand, relays: Vec<String>) -> ActorCommand {
    let protocol = match cmd {
        ActorCommand::Protocol(p) => p,
        other => panic!("expected a Protocol(ResumeSendCommand), got {other:?}"),
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

/// Run a returned reconcile command to completion — it emits no worker command
/// (it only check-states the mint and mutates state), so this drives `run()`
/// then waits a bounded window for its worker thread (mint round-trip + fold)
/// to finish, the same way `check_state_tests` waits on `spawn_debounced`.
fn run_reconcile_and_wait(cmd: ActorCommand) {
    let protocol = match cmd {
        ActorCommand::Protocol(p) => p,
        other => panic!("expected a Protocol(ResumeSendCommand), got {other:?}"),
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

/// swapped-not-signed: the persisted post-swap proofs re-drive `finish_send`.
/// The first worker command is the kind:9321 sign step (never a mint HTTP
/// call) — proving the re-drive resumed straight into `finish_send`.
#[test]
fn swapped_not_signed_restore_re_drives_finish_send() {
    let dir = tempfile::tempdir().unwrap();
    let account = "aa".repeat(32);
    let selected = vec![stored(MINT, proof(30, "02aa", "sec-a"))];
    seed_send(
        dir.path(),
        &account,
        "send-op",
        MINT,
        &selected,
        Some(wal_payload::SwappedSend {
            new_proofs: vec![synthetic_proof(21, "02bb"), synthetic_proof(8, "02cc")],
            nutzap_count: 1,
        }),
    );

    let backend = CashuWalletBackend::with_wal_store(Some(fs_store(dir.path())));
    let mut resumes = backend.restore_from_wal(&account);
    assert_eq!(resumes.len(), 1, "a swapped send must be re-driven");

    match run_resume_first_command(resumes.remove(0), vec!["wss://recipient-relay.example".to_string()]) {
        ActorCommand::Sign(SignCommand::EventForAccount { unsigned, .. }) => {
            assert_eq!(
                unsigned.kind, KIND_NIP61_NUTZAP,
                "the re-drive must resume straight into the kind:9321 sign, no mint HTTP"
            );
        }
        other => panic!("expected the re-drive to resume into finish_send's sign step, got {other:?}"),
    }
}

/// reserved-not-swapped, inputs unspent: the swap never committed, so the send
/// is failed (terminal). The inputs are NOT re-added (they come back through
/// this account's own kind:7375 ingest) and the WAL row is deleted.
#[test]
fn reserved_unspent_restore_fails_the_send() {
    let dir = tempfile::tempdir().unwrap();
    let account = "bb".repeat(32);
    let body = serde_json::json!({
        "states": [{"Y": y_hex("sec-a"), "state": "UNSPENT"}]
    })
    .to_string();
    let mock = spawn_mock_mint(vec![(200, body)]);
    let selected = vec![stored(&mock, proof(30, "02aa", "sec-a"))];
    seed_send(dir.path(), &account, "send-op", &mock, &selected, None);

    let store = fs_store(dir.path());
    let backend = CashuWalletBackend::with_wal_store(Some(Arc::clone(&store)));
    let mut resumes = backend.restore_from_wal(&account);
    assert_eq!(resumes.len(), 1, "a reserved-not-swapped send must be reconciled");

    run_reconcile_and_wait(resumes.remove(0));

    let op = WalletOperationId::new("send-op");
    let state = state::lock_state(&backend.state);
    assert_eq!(
        state.journal.get(&op).map(|o| o.state),
        Some(WalletOperationState::Failed),
        "an all-unspent reconcile fails the send"
    );
    drop(state);
    assert!(
        store.load_operations(&account).unwrap().is_empty(),
        "the terminal Failed transition deletes the durable saga row"
    );
}

/// reserved-not-swapped, an input spent: the swap may have committed and the
/// outputs were lost — the send is left `Unknown` and surfaces in
/// `pending_operations` as needing attention (never silently failed/dropped).
#[test]
fn reserved_spent_restore_leaves_send_unknown_and_surfaced() {
    let dir = tempfile::tempdir().unwrap();
    let account = "cc".repeat(32);
    let body = serde_json::json!({
        "states": [{"Y": y_hex("sec-a"), "state": "SPENT"}]
    })
    .to_string();
    let mock = spawn_mock_mint(vec![(200, body)]);
    let selected = vec![stored(&mock, proof(30, "02aa", "sec-a"))];
    seed_send(dir.path(), &account, "send-op", &mock, &selected, None);

    let backend = CashuWalletBackend::with_wal_store(Some(fs_store(dir.path())));
    let mut resumes = backend.restore_from_wal(&account);
    assert_eq!(resumes.len(), 1);

    run_reconcile_and_wait(resumes.remove(0));

    let op = WalletOperationId::new("send-op");
    assert!(
        wal_send::is_reconcilable_send_state(WalletOperationState::Unknown),
        "Unknown is a reconcilable/surfacing send state"
    );
    let state = state::lock_state(&backend.state);
    assert_eq!(
        state.journal.get(&op).map(|o| o.state),
        Some(WalletOperationState::Unknown),
        "a spent input leaves the send Unknown, not Failed"
    );
    assert!(
        state
            .journal
            .pending_operations()
            .iter()
            .any(|o| o.id == op),
        "the Unknown send stays in pending_operations, surfaced as needing attention"
    );
}
