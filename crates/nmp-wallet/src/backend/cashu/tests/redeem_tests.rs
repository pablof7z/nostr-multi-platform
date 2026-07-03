//! `RedeemNutzap` — W9 (#2917): `RedeemNutzapCommand::run`'s independent
//! verification gates. See `redeem_worker_tests.rs` (AGENTS.md file-size
//! split) for the swap-before-redeem fold + kind:7375/kind:7376 publish
//! (`redeem::finish_redeem`, driven directly with synthetic proofs — the
//! mint HTTP/DHKE round-trip itself is `nmp-nip60`'s own tested surface).

use super::*;
use nmp_nip60::nutzap::{p2pk_secret, NutZapProof};

fn ctx(account_pubkey: Option<&str>) -> WalletBackendContext<'_> {
    WalletBackendContext {
        now_secs: 1_700_000_000,
        selected_backend: None,
        account_pubkey,
    }
}

const ACCOUNT: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";

fn run_redeem(
    backend: &CashuWalletBackend,
    cached: &FixedCachedEvents,
    correlation_id: &str,
    event_id: &str,
) -> RecordingErrorSurface {
    let commands = backend.start_intent(
        ctx(Some(ACCOUNT)),
        WalletIntent::RedeemNutzap {
            event_id: event_id.to_string(),
        },
        Some(correlation_id.to_string()),
    );
    let ActorCommand::Protocol(cmd) = commands.into_iter().next().unwrap() else {
        panic!("expected a Protocol command");
    };
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
    let recipients = FixedRecipientLookup(vec!["wss://my-relay.example".to_string()]);
    let errors = RecordingErrorSurface::default();
    let (worker_tx, _worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    {
        let mut c = ctx_with_cached_events_and_errors(
            &send,
            nmp_core::CommandSender::new(worker_tx),
            &clock,
            &recipients,
            cached,
            &errors,
        );
        cmd.run(&mut c).expect("run returns Ok");
    }
    errors
}

fn expect_error_code(errors: &RecordingErrorSurface, code: &str) {
    assert_eq!(errors.last_token_code.lock().unwrap().as_deref(), Some(code));
}

/// A proof with a P2PK secret locked to `pubkey`, no DLEQ. Every test below
/// that uses this fixture fails closed at an EARLIER gate (unknown event,
/// wrong `p` tag, untrusted mint, no wallet, wrong lock target, ...) than
/// `verify_nutzap_dleq`, which — since #2933 — rejects a missing DLEQ rather
/// than skipping it; this fixture never reaches that check in this file's
/// tests, so `dleq: None` stays a harmless placeholder here, not a bypass.
fn locked_proof(amount: u64, c: &str, pubkey: &str) -> NutZapProof {
    NutZapProof {
        amount,
        id: "keyset-1".to_string(),
        secret: p2pk_secret(pubkey),
        c: c.to_string(),
        dleq: None,
    }
}

#[test]
fn unknown_event_id_fails_closed() {
    let backend = backend_with_mint();
    let errors = run_redeem(
        &backend,
        &FixedCachedEvents::default(),
        "cid-unknown",
        &"e".repeat(64),
    );
    expect_error_code(&errors, ui_codes::INVALID_NUTZAP);
}

#[test]
fn nutzap_not_p_tagged_to_self_fails_closed() {
    let backend = backend_with_mint();
    let sender = nostr::Keys::generate();
    let someone_else = "c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3";
    let proofs = vec![locked_proof(21, "02aa", "unused-lock-target")];
    let event = nutzap_kernel_event(&sender, proofs, MINT, someone_else, None, None, 1_699_999_500);
    let event_id = event.id.clone();
    let cached = FixedCachedEvents(vec![event]);
    let errors = run_redeem(&backend, &cached, "cid-not-self", &event_id);
    expect_error_code(&errors, ui_codes::INVALID_NUTZAP);
}

#[test]
fn untrusted_mint_fails_closed() {
    let backend = backend_with_mint();
    lock_state(&backend.state).cashu_pubkey_hex = Some("02".to_string() + &"55".repeat(32));
    let sender = nostr::Keys::generate();
    let our_pubkey = "02".to_string() + &"55".repeat(32);
    let proofs = vec![locked_proof(21, "02aa", &our_pubkey)];
    let event = nutzap_kernel_event(
        &sender,
        proofs,
        "https://untrusted-mint.example",
        ACCOUNT,
        None,
        None,
        1_699_999_500,
    );
    let event_id = event.id.clone();
    let cached = FixedCachedEvents(vec![event]);
    let errors = run_redeem(&backend, &cached, "cid-untrusted-mint", &event_id);
    expect_error_code(&errors, ui_codes::NO_TRUSTED_MINT);
}

#[test]
fn no_cashu_wallet_fails_closed() {
    let backend = backend_with_mint(); // mint accepted, but no cashu_pubkey_hex set
    let sender = nostr::Keys::generate();
    let proofs = vec![locked_proof(21, "02aa", "unused-lock-target")];
    let event = nutzap_kernel_event(&sender, proofs, MINT, ACCOUNT, None, None, 1_699_999_500);
    let event_id = event.id.clone();
    let cached = FixedCachedEvents(vec![event]);
    let errors = run_redeem(&backend, &cached, "cid-no-wallet", &event_id);
    expect_error_code(&errors, ui_codes::NO_CASHU_WALLET);
}

#[test]
fn proof_not_locked_to_our_pubkey_fails_closed() {
    let backend = backend_with_mint();
    lock_state(&backend.state).cashu_pubkey_hex = Some("02".to_string() + &"66".repeat(32));
    let sender = nostr::Keys::generate();
    // Locked to a DIFFERENT pubkey than the one this wallet holds.
    let proofs = vec![locked_proof(21, "02aa", &("02".to_string() + &"77".repeat(32)))];
    let event = nutzap_kernel_event(&sender, proofs, MINT, ACCOUNT, None, None, 1_699_999_500);
    let event_id = event.id.clone();
    let cached = FixedCachedEvents(vec![event]);
    let errors = run_redeem(&backend, &cached, "cid-wrong-lock", &event_id);
    expect_error_code(&errors, ui_codes::INVALID_NUTZAP);
}

/// A wallet whose `create` flow ran (so `cashu_pubkey_hex` is set) but whose
/// privkey never made it into THIS session's live state (the documented
/// cold-start-recovery deferral) must still fail closed rather than silently
/// skip the P2PK witness.
#[test]
fn missing_privkey_fails_closed() {
    let backend = backend_with_mint();
    let our_pubkey = "02".to_string() + &"88".repeat(32);
    lock_state(&backend.state).cashu_pubkey_hex = Some(our_pubkey.clone());
    // Deliberately no `cashu_privkey` set.
    let sender = nostr::Keys::generate();
    let proofs = vec![locked_proof(21, "02aa", &our_pubkey)];
    let event = nutzap_kernel_event(&sender, proofs, MINT, ACCOUNT, None, None, 1_699_999_500);
    let event_id = event.id.clone();
    let cached = FixedCachedEvents(vec![event]);
    let errors = run_redeem(&backend, &cached, "cid-no-privkey", &event_id);
    expect_error_code(&errors, ui_codes::NO_CASHU_WALLET);
}

/// #2933 end-to-end: the pure `verify_nutzap_dleq_against_keyset` unit tests
/// in `nmp-nip60` prove the fail-closed logic itself, but nothing exercised
/// the FULL `RedeemNutzapCommand::run` path — including the real
/// `verify_nutzap_dleq` mint HTTP round-trip this command actually calls —
/// rejecting a missing-DLEQ nutzap. Every other fixture in this file built
/// from `locked_proof` (which always carries `dleq: None`) fails closed at
/// an EARLIER gate and never reaches the DLEQ check at all (see
/// `locked_proof`'s doc comment), so without this test the command-level
/// wiring from `verify_nutzap_dleq`'s `Err` to `INVALID_NUTZAP` was
/// completely untested. Needs a real (local, mocked) mint HTTP round-trip —
/// `spawn_mock_mint` — and a live `cashu_privkey` (no existing test in this
/// file sets one) so this attempt actually reaches the DLEQ check instead of
/// failing earlier at `NO_CASHU_WALLET` (see #2933's `redeem.rs` reorder).
#[test]
fn missing_dleq_fails_closed_end_to_end_through_the_full_redeem_command() {
    // A REAL secp256k1 point — `build_pubkey_map` (which `verify_nutzap_dleq`
    // calls before ever reaching the per-proof DLEQ check) parses this as an
    // actual curve point, not just well-formed hex; an arbitrary hex string
    // fails there instead, which would make this test pass for the wrong
    // reason (a keyset-parse error, not the DLEQ check this test targets).
    let secp = nostr::secp256k1::Secp256k1::new();
    let mint_sk = nostr::secp256k1::SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
    let mint_pk = nostr::secp256k1::PublicKey::from_secret_key(&secp, &mint_sk);
    let mock_mint = spawn_mock_mint(vec![
        (
            200,
            serde_json::json!({
                "keysets": [{
                    "id": "00mockkeyset",
                    "unit": "sat",
                    "keys": { "21": mint_pk.to_string() },
                }]
            })
            .to_string(),
        ),
        // `/v1/keysets` (fee info merge) is best-effort in `get_sat_keyset`
        // — any error (including this malformed 404) is silently ignored.
        (404, "not found".to_string()),
    ]);
    let backend = backend_with_mint();
    let our_pubkey = "02".to_string() + &"dd".repeat(32);
    {
        let mut state = lock_state(&backend.state);
        state.mints = vec![mock_mint.clone()];
        state.cashu_pubkey_hex = Some(our_pubkey.clone());
        // Unlike every other test in this file, this one needs a REAL
        // privkey present so the flow gets past `NO_CASHU_WALLET` and
        // actually reaches `verify_nutzap_dleq`.
        state.cashu_privkey = Some(state::CashuP2pkSecret(nostr::secp256k1::SecretKey::new(
            &mut nostr::secp256k1::rand::thread_rng(),
        )));
    }
    let sender = nostr::Keys::generate();
    // `dleq: None` — the exact shape #2933 now rejects instead of skipping.
    let proofs = vec![locked_proof(21, "02aa", &our_pubkey)];
    let event = nutzap_kernel_event(&sender, proofs, &mock_mint, ACCOUNT, None, None, 1_699_999_500);
    let event_id = event.id.clone();
    let cached = FixedCachedEvents(vec![event]);
    let errors = run_redeem(&backend, &cached, "cid-missing-dleq-e2e", &event_id);
    expect_error_code(&errors, ui_codes::INVALID_NUTZAP);
}

/// #2942 — the OBSERVER path (`CashuWalletBackend::on_wallet_event`, called
/// exactly this way by `WalletRuntime`'s `WalletEventSink::on_kernel_event` in
/// production — see `runtime.rs`) must dispatch the same `RedeemNutzapCommand`
/// the explicit `nmp.wallet.nutzap.redeem` action reaches, for an inbound
/// `#p`=self kind:9321 the runtime's reconciler observes. Before #2942's fix
/// this path never ran at all for a fresh account (the nutzap-receipts
/// interest opened at the wrong routing scope and got zero relay entries —
/// see `runtime.rs`'s module docs); this test proves that once the event DOES
/// reach the sink (live delivery, or a backfill/EOSE replay — both funnel
/// through the same `ObservedProjectionSink::on_kernel_event` call), a
/// deliberately-unverifiable nutzap (untrusted mint) lands at `Failed` in the
/// journal — visibly rejected, never silently absent — matching the design
/// doc's "Observer counting: unverifiable nutzaps may be shown as
/// rejected/ignored state, not counted as value"
/// (`docs/architecture/nip60-nip61-wallet-design.md`).
#[test]
fn on_wallet_event_dispatches_redeem_and_a_deliberately_unverifiable_nutzap_lands_failed() {
    let backend = backend_with_mint();
    let sender = nostr::Keys::generate();
    let proofs = vec![locked_proof(21, "02aa", "unused-lock-target")];
    let event = nutzap_kernel_event(
        &sender,
        proofs,
        "https://untrusted-mint.example",
        ACCOUNT,
        None,
        None,
        1_699_999_500,
    );
    let event_id = event.id.clone();

    // The exact call `WalletEventSink::on_kernel_event` makes in production
    // (runtime.rs) — NOT `start_intent(RedeemNutzap)`, which is the
    // explicit-action path the other tests in this file exercise.
    let commands = backend.on_wallet_event(ctx(Some(ACCOUNT)), &event);
    let ActorCommand::Protocol(cmd) = commands
        .into_iter()
        .next()
        .expect("on_wallet_event must dispatch a RedeemNutzapCommand for a #p=self kind:9321")
    else {
        panic!("expected a Protocol command");
    };

    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
    let recipients = FixedRecipientLookup(vec!["wss://my-relay.example".to_string()]);
    let errors = RecordingErrorSurface::default();
    let (worker_tx, _worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    let cached = FixedCachedEvents(vec![event]);
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
    expect_error_code(&errors, ui_codes::NO_TRUSTED_MINT);

    let operation_id = super::super::nutzap_dispatch::redeem_operation_id(&event_id);
    let state = lock_state(&backend.state);
    let operation = state.journal.get(&operation_id).expect(
        "the observer-dispatched operation must be recorded in the journal — never \
         silently absent, even for a rejected nutzap",
    );
    assert_eq!(
        operation.state,
        crate::journal::WalletOperationState::Failed,
        "a deliberately-unverifiable (untrusted mint) nutzap must land Failed, not vanish"
    );
}

/// Redeeming the SAME event twice — once already folded as `NutzapRedeemed`
/// — must fail closed on the second attempt rather than double-count.
#[test]
fn already_redeemed_fails_closed() {
    let backend = backend_with_mint();
    let our_pubkey = "02".to_string() + &"99".repeat(32);
    lock_state(&backend.state).cashu_pubkey_hex = Some(our_pubkey.clone());
    let sender = nostr::Keys::generate();
    let proofs = vec![locked_proof(21, "02aa", &our_pubkey)];
    let event = nutzap_kernel_event(&sender, proofs, MINT, ACCOUNT, None, None, 1_699_999_500);
    let event_id = event.id.clone();
    {
        let mut state = lock_state(&backend.state);
        state.ledger.apply(crate::journal::WalletFact::NutzapRedeemed {
            nutzap: crate::journal::WalletEventId::new(event_id.clone()),
            amount_msat: 21_000,
            sender: crate::journal::PubkeyRef::new(sender.public_key().to_hex()),
        });
    }
    let cached = FixedCachedEvents(vec![event]);
    let errors = run_redeem(&backend, &cached, "cid-already", &event_id);
    expect_error_code(&errors, ui_codes::ALREADY_REDEEMED);
}
