//! #3003 — `nutzap.send`'s cross-mint auto-fallback: when NO mutual mint has
//! enough balance (or none is mutual at all) but the recipient lists a mint
//! this wallet can fund via a cross-mint transfer, `SendNutzapCommand::run`
//! must dispatch a `CrossMintTransferCommand` (via `ctx.send`) instead of
//! failing `NO_TRUSTED_MINT`/`INSUFFICIENT_BALANCE` outright.

use super::*;

const ACCOUNT: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const RECIPIENT: &str = "c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3";

fn ctx(account_pubkey: Option<&str>) -> WalletBackendContext<'_> {
    WalletBackendContext {
        now_secs: 1_700_000_000,
        selected_backend: None,
        account_pubkey,
    }
}

/// The recipient accepts a mint this wallet has never even heard of
/// (`state.mints` has nothing in common with it — `any_mutual_mint` stays
/// `false`), but the wallet holds real spendable balance at a DIFFERENT
/// mint. Before #3003 this failed closed with `NO_TRUSTED_MINT`; now it
/// must dispatch a `CrossMintTransferCommand` targeting the recipient's mint
/// instead, and leave THIS send operation `Failed` (superseded) rather than
/// dangling non-terminal forever.
#[test]
fn no_mutual_mint_but_fundable_recipient_mint_dispatches_cross_mint_transfer() {
    const OUR_SOURCE_MINT: &str = "https://our-source-mint.example";
    const RECIPIENT_ONLY_MINT: &str = "https://recipient-only-mint.example";

    let backend = CashuWalletBackend::new();
    {
        let mut state = state::lock_state(&backend.state);
        // This wallet accepts (and holds balance at) a mint the recipient
        // does NOT list at all.
        state.mints = vec![OUR_SOURCE_MINT.to_string()];
        state.add_proofs(
            None,
            OUR_SOURCE_MINT.to_string(),
            vec![synthetic_proof(100, "02aa")],
        );
    }
    let info = nmp_nip60::nutzap::NutZapInfo {
        relays: vec!["wss://recipient-relay.example".to_string()],
        mints: vec![RECIPIENT_ONLY_MINT.to_string()],
        cashu_pubkey: Some("02".to_string() + &"44".repeat(32)),
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
            amount_sats: 21,
            target_event_id: None,
        },
        Some("cid-fallback".to_string()),
    );
    let ActorCommand::Protocol(cmd) = commands.into_iter().next().unwrap() else {
        panic!("expected a Protocol command");
    };

    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
    let recipients = FixedRecipientLookup(Vec::new());
    let errors = RecordingErrorSurface::default();
    let (worker_tx, _worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    {
        let mut c = ctx_with_cached_events_and_errors(
            &send,
            nmp_core::CommandSender::new(worker_tx),
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
        "the fallback must never surface NO_TRUSTED_MINT when a fundable recipient mint exists"
    );
    let sent = sink.sends.lock().unwrap();
    assert_eq!(
        sent.len(),
        1,
        "exactly one CrossMintTransferCommand must be dispatched synchronously via ctx.send"
    );
    assert!(
        matches!(sent[0], ActorCommand::Protocol(_)),
        "expected a Protocol(CrossMintTransferCommand), got {:?}",
        sent[0]
    );

    // The original SendNutzap operation is superseded (Failed, terminal) —
    // never left dangling; the transfer's own re-dispatch will resolve the
    // caller's `cid-fallback` correlation id once it settles.
    let state = state::lock_state(&backend.state);
    let op = state
        .journal
        .get(&crate::journal::WalletOperationId::new("cid-fallback"))
        .expect("original send operation still recorded");
    assert_eq!(op.state, WalletOperationState::Failed);
    // The reservation at the (only) source mint must NOT have been touched
    // by the ORIGINAL send at all — the cross-mint transfer's own worker
    // reserves separately once it runs.
    assert_eq!(
        state.proofs.len(),
        1,
        "no proof was consumed by this send attempt"
    );
}

/// The recipient's mint IS mutual, but this wallet is underfunded there —
/// AND has no other mint at all to fund a transfer from. Must still fail
/// closed with `INSUFFICIENT_BALANCE` exactly as before #3003 (never
/// attempt a transfer with no fundable source).
#[test]
fn mutual_but_underfunded_with_no_other_mint_still_fails_insufficient_balance() {
    const MINT: &str = "https://mint.example";
    let backend = CashuWalletBackend::new();
    {
        let mut state = state::lock_state(&backend.state);
        state.mints = vec![MINT.to_string()];
        // No proofs anywhere.
    }
    let info = nmp_nip60::nutzap::NutZapInfo {
        relays: vec!["wss://recipient-relay.example".to_string()],
        mints: vec![MINT.to_string()],
        cashu_pubkey: Some("02".to_string() + &"44".repeat(32)),
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
            amount_sats: 21,
            target_event_id: None,
        },
        Some("cid-no-fallback".to_string()),
    );
    let ActorCommand::Protocol(cmd) = commands.into_iter().next().unwrap() else {
        panic!("expected a Protocol command");
    };
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
    let recipients = FixedRecipientLookup(Vec::new());
    let errors = RecordingErrorSurface::default();
    let (worker_tx, _worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    {
        let mut c = ctx_with_cached_events_and_errors(
            &send,
            nmp_core::CommandSender::new(worker_tx),
            &clock,
            &recipients,
            &cached,
            &errors,
        );
        cmd.run(&mut c).expect("run returns Ok");
    }
    assert_eq!(
        errors.last_token_code.lock().unwrap().as_deref(),
        Some(ui_codes::INSUFFICIENT_BALANCE)
    );
}
