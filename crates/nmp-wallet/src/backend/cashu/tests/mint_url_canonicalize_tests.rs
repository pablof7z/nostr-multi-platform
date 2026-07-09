//! #2972 — a real-sats `nutzap.send` against `mint.minibits.cash` failed
//! "insufficient balance" despite a funded deposit, because `select_proofs`
//! (and every other mint-URL comparison in this backend) matched by exact
//! string equality: a deposit's stored proof `mint` and a send's resolved
//! `mint` can name the same real mint while differing by scheme/host case or
//! a trailing slash. These tests:
//! - pin down [`nmp_nip60::cashu::canonicalize_mint_url`]'s exact normalization rule
//!   (case/slash collapse, path preserved byte-for-byte);
//! - reproduce the red->green regression directly against
//!   `CashuWalletState::add_proofs`/`select_proofs` (no real mint needed);
//! - reproduce the full `SendNutzapCommand::run` trace from the issue: a
//!   recipient's kind:10019 mint that differs from this wallet's own stored
//!   mint by a trailing slash must still find the deposited proofs.

use super::*;
use nmp_core::actor::SignCommand;

mod canonicalize_mint_url_table {
    use nmp_nip60::cashu::canonicalize_mint_url as c;

    #[test]
    fn trailing_slash_is_stripped() {
        assert_eq!(c("https://mint.example/Bitcoin/"), c("https://mint.example/Bitcoin"));
    }

    #[test]
    fn scheme_and_host_case_is_folded() {
        assert_eq!(
            c("HTTPS://Mint.Minibits.Cash/Bitcoin"),
            c("https://mint.minibits.cash/Bitcoin")
        );
    }

    #[test]
    fn path_case_is_preserved_not_folded() {
        // Cashu mint URLs are path-sensitive (minibits serves a distinct
        // endpoint per unit at e.g. `/Bitcoin`) — differing ONLY in path
        // case must NOT be collapsed to the same canonical string.
        assert_ne!(
            c("https://mint.example/Bitcoin"),
            c("https://mint.example/bitcoin")
        );
    }

    #[test]
    fn different_host_stays_distinct() {
        assert_ne!(
            c("https://mint.example/Bitcoin"),
            c("https://another-mint.example/Bitcoin")
        );
    }

    #[test]
    fn different_path_stays_distinct() {
        assert_ne!(
            c("https://mint.example/Bitcoin"),
            c("https://mint.example/Sat")
        );
    }

    #[test]
    fn no_path_round_trips_unchanged() {
        assert_eq!(c("https://mint.example"), "https://mint.example");
        assert_eq!(c("https://mint.example/"), "https://mint.example");
    }

    #[test]
    fn idempotent() {
        let once = c("HTTPS://Mint.Example/Bitcoin/");
        assert_eq!(c(&once), once);
    }

    #[test]
    fn malformed_input_falls_back_to_trimmed_original() {
        // No `scheme://` separator — never panics, never invents a form.
        assert_eq!(c("  not-a-url  "), "not-a-url");
    }

    /// Only ONE trailing slash is a normalization target (the exact shape a
    /// real client mis-typing/re-typing the same mint produces) — repeated
    /// trailing slashes must stay distinct from the single-slash and
    /// no-slash forms rather than all collapsing together.
    #[test]
    fn repeated_trailing_slashes_are_not_all_collapsed() {
        assert_eq!(c("https://mint.example/Bitcoin//"), "https://mint.example/Bitcoin/");
        assert_ne!(
            c("https://mint.example/Bitcoin//"),
            c("https://mint.example/Bitcoin")
        );
    }

    /// A query string (unlikely for a real mint URL, but must not silently
    /// misbehave) sits past the authority — only the authority is
    /// case-folded, the query is left untouched, and no path-only trailing
    /// slash is invented or stripped inside it.
    #[test]
    fn query_string_is_left_untouched_past_the_authority() {
        assert_eq!(
            c("HTTPS://Mint.Example?Token=ABC"),
            "https://mint.example?Token=ABC"
        );
        assert_ne!(
            c("https://mint.example?Token=ABC"),
            c("https://mint.example?token=abc")
        );
    }
}

/// Step 1 of #2972's fix: reproduce the bug directly against `state.rs`,
/// no send/recipient plumbing involved. Before the fix this returns `None`
/// (BUG) even though the mint is the same one, just spelled with a trailing
/// slash; after the fix it finds the deposited proofs.
#[test]
fn select_proofs_matches_across_a_trailing_slash() {
    let mut state = CashuWalletState::new();
    state.add_proofs(
        None,
        "https://mint.example/Bitcoin".to_string(),
        vec![synthetic_proof(10, "02aa")],
    );

    let found = state.select_proofs("https://mint.example/Bitcoin/", 5);
    assert!(
        found.is_some(),
        "a trailing-slash variant of a stored mint must still resolve the \
         deposited proofs — this is the exact real-sats failure from #2972"
    );
    let (selected, total) = found.unwrap();
    assert_eq!(total, 10);
    assert_eq!(selected.len(), 1);
}

/// Same red->green, the other direction: proofs stored under a scheme/host
/// case variant must still be found by the lowercase lookup form.
#[test]
fn select_proofs_matches_across_host_case() {
    let mut state = CashuWalletState::new();
    state.add_proofs(
        None,
        "HTTPS://Mint.Example/Bitcoin".to_string(),
        vec![synthetic_proof(10, "02bb")],
    );

    let found = state.select_proofs("https://mint.example/Bitcoin", 10);
    assert!(found.is_some());
}

/// A genuinely different mint (different path/unit) must never be treated
/// as the same one — canonicalization must not become a way to overspend
/// against the wrong mint.
#[test]
fn select_proofs_does_not_collapse_a_different_path() {
    let mut state = CashuWalletState::new();
    state.add_proofs(
        None,
        "https://mint.example/Bitcoin".to_string(),
        vec![synthetic_proof(10, "02cc")],
    );

    assert!(state.select_proofs("https://mint.example/Sat", 5).is_none());
}

/// End-to-end reproduction of the issue's exact trace: a wallet whose own
/// `mints` allow-list holds one string form deposits proofs under that same
/// form, then a recipient's kind:10019 advertises the SAME real mint with a
/// trailing slash. Before the fix this fails `INSUFFICIENT_BALANCE` (or even
/// `NO_TRUSTED_MINT`, if the intersection check itself missed); after the
/// fix `SendNutzapCommand::run` must get past both gates onto `MintPending`.
#[test]
fn send_finds_deposited_proofs_when_recipient_mint_tag_has_a_trailing_slash() {
    const OUR_MINT: &str = "https://mint.example/Bitcoin";
    let backend = CashuWalletBackend::new();
    {
        let mut state = state::lock_state(&backend.state);
        state.mints = vec![OUR_MINT.to_string()];
        // Mirrors a completed deposit's `add_proofs` call (`token_event.rs`).
        state.add_proofs(
            Some("deposit-token-event".to_string()),
            OUR_MINT.to_string(),
            vec![synthetic_proof(10, "02dd")],
        );
    }

    let info = nmp_nip60::nutzap::NutZapInfo {
        relays: vec!["wss://recipient-relay.example".to_string()],
        // The recipient's own client publishes a trailing-slash form of the
        // exact same mint — this is the #2972 real-mint mismatch.
        mints: vec![format!("{OUR_MINT}/")],
        cashu_pubkey: Some("02".to_string() + &"44".repeat(32)),
    };
    const RECIPIENT: &str = "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";
    let cached = FixedCachedEvents(vec![nutzap_info_kernel_event(RECIPIENT, &info, 1_699_999_000)]);

    let commands = backend.start_intent(
        WalletBackendContext {
            now_secs: 1_700_000_000,
            selected_backend: None,
            account_pubkey: Some("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"),
        },
        WalletIntent::SendNutzap {
            recipient_pubkey: RECIPIENT.to_string(),
            amount_sats: 5,
            target_event_id: None,
        },
        Some("cid-2972".to_string()),
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
        "must not fail closed — the recipient's trailing-slash mint form is \
         the same real mint this wallet deposited to"
    );
    let state = state::lock_state(&backend.state);
    let op = state
        .journal
        .get(&crate::journal::WalletOperationId::new("cid-2972"))
        .unwrap();
    assert_eq!(
        op.state,
        crate::journal::WalletOperationState::MintPending,
        "proof selection must succeed and reserve the selection before the \
         (untested-here) mint HTTP round-trip"
    );
}

/// The proof inventory (`add_proofs`) is canonicalized, but a deposit's
/// ledger fact (`WalletFact::TokenAdded`'s `mint` key, which `snapshot()`'s
/// `balances()` groups by) is built from a SEPARATE clone of the same
/// caller-supplied string — this must be canonicalized too, or two deposits
/// to the same real mint spelled two different ways would report as two
/// separate balance rows even though `select_proofs` already treats them as
/// one spendable pool. Drives `dispatch_token_event` directly (mirrors
/// `deposit_tests::dispatch_token_event_applies_token_added_and_settles_the_operation`)
/// with a trailing-slash mint string and asserts the resulting balance is
/// addressable by the CANONICAL `MintUrl`, not the raw one.
#[test]
fn dispatch_token_event_canonicalizes_the_ledger_facts_mint_key_too() {
    const RAW_MINT: &str = "HTTPS://Mint.Example/Bitcoin/";
    let backend = backend_with_mint();
    let operation_id = crate::journal::WalletOperationId::new("cid-ledger-key");
    {
        let mut state = state::lock_state(&backend.state);
        state
            .begin_operation(operation_id.clone(), WalletOperationKind::DepositCashu)
            .unwrap();
        state
            .transition(&operation_id, crate::journal::WalletOperationState::MintPending)
            .unwrap();
        state
            .transition(&operation_id, crate::journal::WalletOperationState::MintSettled)
            .unwrap();
        state.pending_deposits.insert(
            "quote-ledger-key".to_string(),
            state::PendingDeposit {
                operation_id: operation_id.clone(),
                mint: RAW_MINT.to_string(),
                amount_sats: 10,
                minted_proofs: None,
                signed_token: None,
                chain_started_at: Some(1_700_000_000),
            },
        );
    }
    let account = "ee".repeat(32);
    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    deposit::dispatch_token_event(
        nmp_core::CommandSender::new(worker_tx),
        std::sync::Arc::clone(&backend.state),
        operation_id.clone(),
        "quote-ledger-key".to_string(),
        RAW_MINT.to_string(),
        vec![synthetic_proof(10, "02ee")],
        account.clone(),
        vec!["wss://relay.example".to_string()],
        1_700_000_000,
        Some("cid-ledger-key".to_string()),
    );

    let encrypt_continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(SignCommand::Nip44EncryptForAccount { continuation, .. }) => continuation,
        other => panic!("expected Nip44EncryptForAccount, got {other:?}"),
    };
    encrypt_continuation.call(Ok("fake-token-ciphertext".to_string()));
    let sign_continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(SignCommand::EventForAccount { continuation, .. }) => continuation,
        other => panic!("expected EventForAccount, got {other:?}"),
    };
    sign_continuation.call(Ok(nmp_signer_iface::SignedEvent {
        id: "f".repeat(64),
        sig: "s".repeat(128),
        unsigned: nmp_signer_iface::UnsignedEvent {
            pubkey: account,
            kind: nmp_nip60::KIND_NIP60_TOKEN,
            tags: Vec::new(),
            content: "fake-token-ciphertext".to_string(),
            created_at: 0,
        },
    }));

    let state = state::lock_state(&backend.state);
    let canonical = nmp_nip60::cashu::canonicalize_mint_url(RAW_MINT);
    assert_ne!(
        canonical, RAW_MINT,
        "test fixture must actually exercise a non-canonical raw string"
    );
    assert_eq!(
        state.ledger.state().balance(
            &crate::journal::MintUrl::new(canonical),
            &crate::journal::WalletUnit::new("sat")
        ),
        10_000,
        "the ledger's balance must be addressable by the CANONICAL mint key, \
         not the raw one this deposit happened to be typed with"
    );
}
