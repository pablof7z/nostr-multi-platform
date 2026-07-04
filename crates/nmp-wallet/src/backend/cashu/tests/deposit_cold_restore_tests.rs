//! Cold-restart deposit recovery (PR-2 of #2910): drop the whole in-memory
//! backend, reconstruct a fresh one over the same on-disk WAL, and prove the
//! deposit resumes correctly from each of the three crash points —
//!
//! - (a) quote created only: `pending_deposits` is repopulated (unbreaking
//!   `start_complete_deposit`'s `UNKNOWN_QUOTE` lookup), but NOTHING is
//!   re-driven (the user may not have paid the invoice yet).
//! - (b) minted, not signed: the encrypt/sign/publish chain is re-driven from
//!   the persisted proofs WITHOUT re-touching the mint (a Cashu mint
//!   permanently refuses an already-`ISSUED` quote — this is the actual #2910
//!   money-safety fix).
//! - (c) signed, not yet ingested-as-settled: the EXACT cached kind:7375 is
//!   republished (no re-mint, no re-sign), even from an empty cold-start proof
//!   inventory.
//!
//! Plus the settle rule: once that republished kind:7375 is re-observed from a
//! relay (the #2965 self-authored ingest path), the deposit's WAL entry —
//! saga row AND resume payload — is actually deleted, retiring the formerly
//! unbounded `pending_deposits` map.

use std::sync::Arc;

use super::*;
use nmp_core::actor::{PublishCommand, SignCommand};
use nmp_core::substrate::KernelEvent;
use nmp_nip60::cashu::types::Proof;
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

use crate::journal::{
    FsWalletWalStore, WalletOperationId, WalletOperationKind, WalletOperationState, WalletWalStore,
};

fn fs_store(dir: &std::path::Path) -> Arc<dyn WalletWalStore> {
    Arc::new(FsWalletWalStore::new(dir)) as Arc<dyn WalletWalStore>
}

fn wallet_ctx(account_pubkey: Option<&str>) -> WalletBackendContext<'_> {
    WalletBackendContext {
        now_secs: 1_700_000_000,
        selected_backend: None,
        account_pubkey,
    }
}

/// A signed kind:7375 fixture whose id is `event_id`, authored by `account`.
fn signed_token(account: &str, event_id: &str) -> SignedEvent {
    SignedEvent {
        id: event_id.to_string(),
        sig: "s".repeat(128),
        unsigned: UnsignedEvent {
            pubkey: account.to_string(),
            kind: nmp_nip60::KIND_NIP60_TOKEN,
            tags: Vec::new(),
            content: "already-signed-ciphertext".to_string(),
            created_at: 1_700_000_000,
        },
    }
}

/// Write a deposit at a given stage into the durable WAL under `account`, then
/// drop the backend — the "process that crashed" whose only surviving trace is
/// the on-disk WAL. `minted_proofs`/`signed_token` select the crash point.
fn seed_deposit(
    dir: &std::path::Path,
    account: &str,
    quote_id: &str,
    op_id: &str,
    minted_proofs: Option<Vec<Proof>>,
    signed_token: Option<SignedEvent>,
) {
    let backend = CashuWalletBackend::with_wal_store(Some(fs_store(dir)));
    // Arms write-through by setting `wal_account`.
    let _ = backend.restore_from_wal(account);
    let mut state = state::lock_state(&backend.state);
    let op = WalletOperationId::new(op_id);
    state
        .begin_operation(op.clone(), WalletOperationKind::DepositCashu)
        .unwrap();
    state
        .transition(&op, WalletOperationState::MintPending)
        .unwrap();
    state
        .transition(&op, WalletOperationState::MintSettled)
        .unwrap();
    if signed_token.is_some() {
        // The signed path always leaves the operation at `PublishPending`
        // (see `token_event.rs`'s `on_signed`) before the crash.
        state
            .transition(&op, WalletOperationState::PublishPending)
            .unwrap();
    }
    state.pending_deposits.insert(
        quote_id.to_string(),
        state::PendingDeposit {
            operation_id: op,
            mint: MINT.to_string(),
            amount_sats: 15,
            minted_proofs,
            chain_started_at: None,
            signed_token,
        },
    );
    wal_payload::persist_deposit_payload(&state, quote_id);
}

/// Run a returned `ResumeDepositCommand` and return the FIRST command its
/// worker thread emits — enough to prove which chain tail it resumed into.
fn run_resume_first_command(
    cmd: ActorCommand,
    relays: Vec<String>,
) -> ActorCommand {
    let protocol = match cmd {
        ActorCommand::Protocol(p) => p,
        other => panic!("expected a Protocol(ResumeDepositCommand), got {other:?}"),
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

/// (a) A quote-created-only deposit is repopulated on restore so
/// `start_complete_deposit` can find it again — but is NOT re-driven.
#[test]
fn quote_only_restore_repopulates_pending_without_re_driving() {
    let dir = tempfile::tempdir().unwrap();
    let account = "aa".repeat(32);
    seed_deposit(dir.path(), &account, "quote-a", "op-a", None, None);

    // Process restart: brand-new backend over the same on-disk WAL.
    let backend = CashuWalletBackend::with_wal_store(Some(fs_store(dir.path())));
    // Before restore, a completion attempt would hit UNKNOWN_QUOTE.
    let resumes = backend.restore_from_wal(&account);

    assert!(
        resumes.is_empty(),
        "a quote-created-only deposit must not be re-driven"
    );
    let state = state::lock_state(&backend.state);
    assert!(
        state.pending_deposits.contains_key("quote-a"),
        "pending deposit repopulated so start_complete_deposit no longer returns UNKNOWN_QUOTE"
    );
    let pending = &state.pending_deposits["quote-a"];
    assert_eq!(pending.mint, MINT);
    assert_eq!(pending.amount_sats, 15);
    assert!(pending.minted_proofs.is_none());
    assert!(pending.signed_token.is_none());
}

/// (b) A minted-but-not-signed deposit resumes the encrypt/sign/publish chain
/// from the persisted proofs WITHOUT re-touching the mint — the actual #2910
/// fix. The first worker command is the NIP-44 encrypt step (never a mint HTTP
/// call), proving the re-drive went straight into `dispatch_token_event`.
#[test]
fn minted_not_signed_restore_re_drives_the_sign_chain_without_re_minting() {
    let dir = tempfile::tempdir().unwrap();
    let account = "bb".repeat(32);
    seed_deposit(
        dir.path(),
        &account,
        "quote-b",
        "op-b",
        Some(vec![synthetic_proof(15, "02cc")]),
        None,
    );

    let backend = CashuWalletBackend::with_wal_store(Some(fs_store(dir.path())));
    let mut resumes = backend.restore_from_wal(&account);
    assert_eq!(resumes.len(), 1, "a minted deposit must be re-driven");

    // Repopulated with the persisted proofs.
    {
        let state = state::lock_state(&backend.state);
        let pending = &state.pending_deposits["quote-b"];
        assert_eq!(
            pending.minted_proofs.as_ref().map(|p| p.len()),
            Some(1),
            "minted proofs survived the restart"
        );
    }

    match run_resume_first_command(resumes.remove(0), vec!["wss://relay.example".to_string()]) {
        ActorCommand::Sign(SignCommand::Nip44EncryptForAccount { .. }) => {}
        other => panic!(
            "expected the re-drive to resume straight into the encrypt step (no mint HTTP), got {other:?}"
        ),
    }
}

/// (c) A signed-but-not-yet-ingested deposit republishes the EXACT cached
/// kind:7375 on restore — no re-mint, no re-sign — even though the cold-start
/// proof inventory is empty (the crash lost it). Unlike the in-process retry
/// path, the cold re-drive does NOT gate on `still_held` (see
/// `resume.rs`'s module docs).
#[test]
fn signed_not_ingested_restore_republishes_the_cached_event() {
    let dir = tempfile::tempdir().unwrap();
    let account = "cc".repeat(32);
    let event_id = "d".repeat(64);
    seed_deposit(
        dir.path(),
        &account,
        "quote-c",
        "op-c",
        Some(vec![synthetic_proof(15, "02dd")]),
        Some(signed_token(&account, &event_id)),
    );

    let backend = CashuWalletBackend::with_wal_store(Some(fs_store(dir.path())));
    // Cold start: no proofs in inventory (the crash lost them).
    assert!(state::lock_state(&backend.state).proofs.is_empty());

    let mut resumes = backend.restore_from_wal(&account);
    assert_eq!(resumes.len(), 1, "a signed deposit must be re-driven");

    match run_resume_first_command(resumes.remove(0), vec!["wss://relay.example".to_string()]) {
        ActorCommand::Publish(PublishCommand::SignedEvent { raw, .. }) => {
            assert_eq!(
                raw.id, event_id,
                "must republish the SAME cached kind:7375, not a new one"
            );
        }
        other => panic!(
            "expected the re-drive to republish the cached event (no mint, no re-sign), got {other:?}"
        ),
    }
}

/// The settle rule: once the deposit's own kind:7375 is re-observed from a
/// relay, its WAL entry — saga row AND resume payload — is actually deleted,
/// and the `pending_deposits` entry is dropped. This is the load-bearing
/// assertion that the unbounded-map tradeoff is retired.
#[test]
fn ingested_token_settles_the_deposit_and_deletes_its_wal_entry() {
    let dir = tempfile::tempdir().unwrap();
    let account = "ee".repeat(32);
    let event_id = "f".repeat(64);
    let op_id = "op-settle";
    seed_deposit(
        dir.path(),
        &account,
        "quote-settle",
        op_id,
        Some(vec![synthetic_proof(15, "02ee")]),
        Some(signed_token(&account, &event_id)),
    );

    // Restore over a store handle we keep, so we can inspect it after settle.
    let store = fs_store(dir.path());
    let backend = CashuWalletBackend::with_wal_store(Some(Arc::clone(&store)));
    let _ = backend.restore_from_wal(&account);

    // Precondition: the WAL still holds this deposit's row + payload.
    let operation_id = WalletOperationId::new(op_id);
    assert_eq!(store.load_operations(&account).unwrap().len(), 1);
    assert!(store
        .load_payload(&account, &operation_id)
        .unwrap()
        .is_some());
    assert!(state::lock_state(&backend.state)
        .pending_deposits
        .contains_key("quote-settle"));

    // The account's own kind:7375 comes back from a relay — the publish-ACK
    // the deposit flow never otherwise had.
    let token_event = KernelEvent {
        id: event_id.clone(),
        author: account.clone(),
        kind: nmp_nip60::kinds::KIND_NIP60_TOKEN,
        created_at: 1_700_000_050,
        tags: Vec::new(),
        content: "self-authored-ciphertext".to_string(),
        relay_provenance: vec!["wss://relay.example".to_string()],
    };
    let _ = backend.on_wallet_event(wallet_ctx(Some(&account)), &token_event);

    // The deposit is settled and forgotten in memory.
    {
        let state = state::lock_state(&backend.state);
        assert!(
            !state.pending_deposits.contains_key("quote-settle"),
            "settle rule drops the pending_deposits entry (unbounded-map tradeoff retired)"
        );
        assert_eq!(
            state.journal.get(&operation_id).map(|op| op.state),
            Some(WalletOperationState::Settled),
        );
    }

    // ...and its durable WAL entry — row AND payload — is actually gone.
    assert!(
        store.load_operations(&account).unwrap().is_empty(),
        "the saga row is deleted on the terminal Settled transition"
    );
    assert!(
        store
            .load_payload(&account, &operation_id)
            .unwrap()
            .is_none(),
        "the resume payload is deleted on the terminal Settled transition"
    );
}
