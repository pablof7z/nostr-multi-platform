//! #3008 — the full proof that `nutzap.send`'s cross-mint auto-fallback is
//! SELF-SUFFICIENT: the fallback must fund the target mint with MORE than
//! `amount_sats` (a real, keyset-derived fee headroom), and the re-dispatched
//! send must actually reach `Settled` — not underflow `INSUFFICIENT_BALANCE`
//! the way it did before this fix (the target mint held EXACTLY
//! `amount_sats`, so `gross_change` was always `0`, and no nonzero swap fee
//! could ever be covered). See `cross_mint_headroom_tests.rs` for the
//! STANDALONE action's twin proof (exactly `amount_sats`, never headroom).
//!
//! Driven through two REAL local mock mints (`cross_mint_headroom_fallback_
//! support.rs`, split out purely for file-size discipline) — no canned "and
//! assume it works" shortcuts. Every mint response is either read straight
//! back off the incoming request (dynamic echo) or mint-decided nonsense:
//! this suite never hardcodes the implementation's own headroom constant, so
//! it stays valid however that constant is tuned, as long as it is enough to
//! cover the real fee (which is itself asserted, not assumed).

use super::cross_mint_headroom_fallback_support::{
    ctx, spawn_source_mint, spawn_target_mint, ACCOUNT, AMOUNT_SATS, RECIPIENT, TARGET_QUOTE_ID,
};
use super::*;
use std::sync::{Arc, Mutex};

/// The full proof: the recipient lists a mint this wallet cannot currently
/// fund at exact-amount (no mutual mint at all), but CAN fund via a
/// cross-mint transfer from a different mint it holds balance at. The
/// fallback must (1) fund the target mint with MORE than `amount_sats`
/// (headroom, sized off the target's own nonzero `input_fee_ppk`), and (2)
/// the re-dispatched send must actually SETTLE — pre-#3008 it would
/// underflow `INSUFFICIENT_BALANCE` here, because the target mint held
/// EXACTLY `amount_sats` and `gross_change` (`0`) could never cover any
/// nonzero swap fee.
#[test]
fn fallback_transfer_sizes_headroom_so_retried_send_settles_and_surfaces_fee() {
    let captured_funded_amount: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(None));
    let target_mint_url = spawn_target_mint(Arc::clone(&captured_funded_amount));
    let source_mint_url = spawn_source_mint();

    let backend = CashuWalletBackend::new();
    {
        let mut state = state::lock_state(&backend.state);
        // No mutual mint at all — forces the cross-mint path (mirrors
        // `send_cross_mint_fallback_tests.rs`'s own no-mutual-mint case).
        state.mints = Vec::new();
        state.add_proofs(
            None,
            source_mint_url.clone(),
            vec![synthetic_proof(
                100_000,
                &("02".to_string() + &"aa".repeat(32)),
            )],
        );
    }
    let info = nmp_nip60::nutzap::NutZapInfo {
        relays: vec!["wss://recipient-relay.example".to_string()],
        mints: vec![target_mint_url.clone()],
        cashu_pubkey: Some("02".to_string() + &"bb".repeat(32)),
    };
    let cached = FixedCachedEvents(vec![nutzap_info_kernel_event(
        RECIPIENT,
        &info,
        1_699_999_000,
    )]);

    let commands = backend.start_intent(
        ctx(Some(ACCOUNT)),
        WalletIntent::SendNutzap {
            recipient_pubkey: RECIPIENT.to_string(),
            amount_sats: AMOUNT_SATS,
            target_event_id: None,
        },
        Some("cid-headroom".to_string()),
    );
    let ActorCommand::Protocol(cmd) = commands.into_iter().next().unwrap() else {
        panic!("expected a Protocol command");
    };

    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
    let recipients = FixedRecipientLookup(Vec::new());
    let errors = RecordingErrorSurface::default();
    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    {
        let mut c = ctx_with_cached_events_and_errors(
            &send,
            nmp_core::CommandSender::new(worker_tx.clone()),
            &clock,
            &recipients,
            &cached,
            &errors,
        );
        cmd.run(&mut c).expect("run returns Ok");
    }
    assert_eq!(
        errors.last_token_code.lock().unwrap().as_deref(),
        None,
        "the fallback must never surface an error when a fundable recipient mint exists"
    );
    let cross_mint_cmd = {
        let mut sends = sink.sends.lock().unwrap();
        assert_eq!(sends.len(), 1, "expected exactly one dispatched command");
        sends.remove(0)
    };
    let ActorCommand::Protocol(cross_mint_cmd) = cross_mint_cmd else {
        panic!("expected a Protocol(CrossMintTransferCommand)");
    };

    // Drive the cross-mint transfer's own worker thread (target mint-quote
    // -> source melt-quote -> melt -> target mint-quote-status -> mint ->
    // kind:7375 encrypt/sign/publish) all the way to the retry dispatch.
    {
        let mut c = ctx_with_sender(
            &send,
            nmp_core::CommandSender::new(worker_tx.clone()),
            &clock,
            &recipients,
        );
        cross_mint_cmd.run(&mut c).expect("run returns Ok");
    }

    let encrypt_continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(nmp_core::actor::SignCommand::Nip44EncryptForAccount {
            continuation,
            ..
        }) => continuation,
        other => panic!("expected Nip44EncryptForAccount, got {other:?}"),
    };
    encrypt_continuation.call(Ok("fake-token-ciphertext".to_string()));

    let sign_continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(nmp_core::actor::SignCommand::EventForAccount {
            unsigned,
            continuation,
            ..
        }) => {
            assert_eq!(unsigned.kind, nmp_nip60::KIND_NIP60_TOKEN);
            continuation
        }
        other => panic!("expected EventForAccount (kind:7375 sign), got {other:?}"),
    };
    sign_continuation.call(Ok(nmp_signer_iface::SignedEvent {
        id: "1".repeat(64),
        sig: "2".repeat(128),
        unsigned: nmp_signer_iface::UnsignedEvent {
            pubkey: ACCOUNT.to_string(),
            kind: nmp_nip60::KIND_NIP60_TOKEN,
            tags: Vec::new(),
            content: String::new(),
            created_at: 1_700_000_000,
        },
    }));

    // `dispatch_cross_mint_token_event`'s `on_signed` dispatches the
    // re-drived `SendNutzap` FIRST, then enqueues the kind:7375 publish.
    let retry_cmd = match recv_command(&worker_rx) {
        ActorCommand::Protocol(cmd) => cmd,
        other => panic!("expected the re-dispatched SendNutzapCommand, got {other:?}"),
    };
    match recv_command(&worker_rx) {
        ActorCommand::Publish(_) => {}
        other => panic!("expected the kind:7375 publish, got {other:?}"),
    }

    assert!(
        captured_funded_amount
            .lock()
            .unwrap()
            .expect("funded amount captured")
            > AMOUNT_SATS,
        "the fallback must fund MORE than the bare nutzap amount (fee headroom)"
    );

    // Drive the retried send's own worker (get_sat_keyset -> swap -> sign ->
    // publish) — THE underflow this fix closes: without headroom, the
    // target mint would hold EXACTLY `AMOUNT_SATS` and this send would fail
    // `INSUFFICIENT_BALANCE` before ever reaching the mint's `/v1/swap`.
    {
        let mut c = ctx_with_cached_events(
            &send,
            nmp_core::CommandSender::new(worker_tx),
            &clock,
            &recipients,
            &cached,
        );
        retry_cmd.run(&mut c).expect("run returns Ok");
    }
    let retry_sign_continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(nmp_core::actor::SignCommand::EventForAccount {
            unsigned,
            continuation,
            ..
        }) => {
            assert_eq!(unsigned.kind, nmp_nip60::kinds::KIND_NIP61_NUTZAP);
            continuation
        }
        other => panic!("expected the retried send's kind:9321 sign, got {other:?}"),
    };
    retry_sign_continuation.call(Ok(nmp_signer_iface::SignedEvent {
        id: "3".repeat(64),
        sig: "4".repeat(128),
        unsigned: nmp_signer_iface::UnsignedEvent {
            pubkey: ACCOUNT.to_string(),
            kind: nmp_nip60::kinds::KIND_NIP61_NUTZAP,
            tags: Vec::new(),
            content: String::new(),
            created_at: 1_700_000_000,
        },
    }));
    match recv_command(&worker_rx) {
        ActorCommand::Publish(_) => {}
        other => panic!("expected the kind:9321 publish, got {other:?}"),
    }

    // The retried send actually SETTLED.
    let retry_op_id =
        crate::journal::WalletOperationId::new(format!("cross-mint-retry-send-{TARGET_QUOTE_ID}"));
    {
        let state = state::lock_state(&backend.state);
        let op = state
            .journal
            .get(&retry_op_id)
            .expect("retried send operation recorded");
        assert_eq!(
            op.state,
            WalletOperationState::Settled,
            "the headroom must be enough for the retried send to settle, not underflow \
             INSUFFICIENT_BALANCE"
        );
        assert_eq!(
            op.recorded_cross_mint_source.as_deref(),
            Some(source_mint_url.as_str()),
            "the retried send's journal row must carry the melt's SOURCE mint (#3008)"
        );
        let fee_paid = op
            .recorded_fee_sats
            .expect("this send's own swap fee must be recorded");
        assert!(
            fee_paid > 0,
            "the target mint's nonzero input_fee_ppk must produce a nonzero swap fee"
        );
        assert_eq!(
            op.recorded_cross_mint_fee_sats,
            Some(0),
            "the melt itself used fee_reserve: 0, so it contributed no fee"
        );
    }

    // Deliverable 2, read back through the actual product surface
    // (`snapshot()`'s `recent_history`), not just the raw journal fields.
    let snapshot = backend.snapshot(WalletProjectionScope::default());
    let row = snapshot
        .projection
        .recent_history
        .iter()
        .find(|row| row.operation_id == retry_op_id.as_str())
        .expect("the settled retry must surface exactly one recent_history row");
    assert_eq!(row.kind, crate::projection::WalletHistoryKind::SendNutzap);
    assert_eq!(
        row.amount, AMOUNT_SATS,
        "the user-visible amount is the ORIGINAL request"
    );
    assert_eq!(row.source_mint.as_deref(), Some(source_mint_url.as_str()));
    assert_eq!(row.target_mint.as_deref(), Some(target_mint_url.as_str()));
    let fee_paid_sats = row.fee_paid_sats.expect("fee_paid_sats must be surfaced");
    assert!(fee_paid_sats > 0);
}
