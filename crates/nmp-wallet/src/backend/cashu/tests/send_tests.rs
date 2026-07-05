//! `SendNutzap` — W8 (#2917): `SendNutzapCommand::run`'s fail-closed
//! recipient/mint/balance verification gates. `send::finish_send` (the
//! post-swap fold + kind:9321 publish, driven directly with synthetic
//! proofs — the mint HTTP/DHKE round-trip itself is `nmp-nip60`'s own tested
//! surface) moved to `send_worker_tests.rs` (AGENTS.md file-size discipline,
//! mirrors `redeem_tests.rs`/`redeem_worker_tests.rs`'s identical split).

use super::*;
use nmp_core::actor::InterestsCommand;
use nmp_nip60::kinds::KIND_NIP61_NUTZAP_INFO;

fn ctx(account_pubkey: Option<&str>) -> WalletBackendContext<'_> {
    WalletBackendContext {
        now_secs: 1_700_000_000,
        selected_backend: None,
        account_pubkey,
    }
}

const ACCOUNT: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const RECIPIENT: &str = "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";

fn run_send(
    backend: &CashuWalletBackend,
    cached: &FixedCachedEvents,
    correlation_id: &str,
    amount_sats: u64,
) -> RecordingErrorSurface {
    let commands = backend.start_intent(
        ctx(Some(ACCOUNT)),
        WalletIntent::SendNutzap {
            recipient_pubkey: RECIPIENT.to_string(),
            amount_sats,
            target_event_id: None,
        },
        Some(correlation_id.to_string()),
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
            cached,
            &errors,
        );
        cmd.run(&mut c).expect("run returns Ok");
    }
    errors
}

fn expect_error_code(errors: &RecordingErrorSurface, code: &str) {
    assert_eq!(
        errors.last_token_code.lock().unwrap().as_deref(),
        Some(code)
    );
}

/// #3010 — a cache miss on the recipient's kind:10019 no longer fails this
/// attempt closed synchronously: it supersedes the operation (journal-only,
/// no `ShowErrorToken`/`RecordActionFailure`) and parks a continuation that
/// `nutzap_await` redrives once the info arrives (see
/// `send_nutzap_await_tests.rs` for the end-to-end redrive/TTL acceptance
/// tests). Only the bounded TTL sweep ever surfaces
/// `NO_RECIPIENT_NUTZAP_INFO` to a caller now.
#[test]
fn no_recipient_info_parks_instead_of_failing_closed() {
    let backend = backend_with_mint();
    let errors = run_send(&backend, &FixedCachedEvents::default(), "cid-no-info", 21);
    assert_eq!(
        errors.last_token_code.lock().unwrap().as_deref(),
        None,
        "a miss must not surface an error synchronously — it parks instead"
    );
    let state = state::lock_state(&backend.state);
    let op = state
        .journal
        .get(&crate::journal::WalletOperationId::new("cid-no-info"))
        .expect("the original operation must still be recorded");
    assert_eq!(
        op.state,
        WalletOperationState::Failed,
        "the original attempt is superseded (journal-only) at park time"
    );
}

/// #3010 — exactly one `PendingSendAwait` is parked for the recipient on a
/// miss, so `nutzap_await`'s ingest parser has something to redrive once the
/// info arrives.
#[test]
fn no_recipient_info_parks_a_pending_send_await() {
    let backend = backend_with_mint();
    let _errors = run_send(&backend, &FixedCachedEvents::default(), "cid-parks", 21);
    let parked = state::lock_state(&backend.state).take_send_awaits(RECIPIENT);
    assert_eq!(parked.len(), 1, "exactly one await must be parked");
    assert_eq!(parked[0].recipient_pubkey, RECIPIENT);
    assert_eq!(parked[0].amount_sats, 21);
    assert_eq!(parked[0].correlation_id.as_deref(), Some("cid-parks"));
}

/// #2966 — before this fix, `snapshot.rs`'s `history_row` read a
/// `SendNutzap` history row's `amount` from `op.consumed_inputs.last()`,
/// which is empty for any failure this early (proof selection never runs
/// without a resolved recipient) — the nutsack TUI's nutzap feed showed
/// `amount: 0` for every outgoing send regardless of what was actually
/// requested. `start_send_nutzap` now records the intended send amount
/// (`WalletOperation::recorded_amount`) at dispatch time, before recipient
/// resolution ever runs, so even this earliest failure keeps the real
/// amount.
#[test]
fn history_row_shows_the_intended_send_amount_even_when_the_send_fails_before_proof_selection() {
    let backend = backend_with_mint();
    let _errors = run_send(
        &backend,
        &FixedCachedEvents::default(),
        "cid-amount-fidelity",
        4_200,
    );

    let snapshot = backend.snapshot(WalletProjectionScope::default());
    let history_row = snapshot
        .projection
        .recent_history
        .iter()
        .find(|row| row.operation_id == "cid-amount-fidelity")
        .expect("a failed SendNutzap must still surface a history row");
    assert_eq!(
        history_row.kind,
        crate::projection::WalletHistoryKind::SendNutzap
    );
    assert_eq!(history_row.amount, 4_200);
    assert_eq!(history_row.unit, "sat");
    assert_eq!(history_row.state, "Failed");
}

/// #2936 — a cache miss on the recipient's kind:10019 must not just fail
/// closed silently: it opens a warm-the-cache read interest for that
/// specific recipient (`interests::recipient_nutzap_info_interest`) so the
/// event actually flows into the kernel's cache. #3010 additionally parks a
/// continuation (see `no_recipient_info_parks_a_pending_send_await`) so
/// arrival redrives automatically — a caller's own manual retry (this test's
/// scenario) still works too, as an independent, still-supported path.
#[test]
fn no_recipient_info_opens_a_warm_cache_interest() {
    let backend = backend_with_mint();
    let commands = backend.start_intent(
        ctx(Some(ACCOUNT)),
        WalletIntent::SendNutzap {
            recipient_pubkey: RECIPIENT.to_string(),
            amount_sats: 21,
            target_event_id: None,
        },
        Some("cid-warm-cache".to_string()),
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
    let cached = FixedCachedEvents::default();
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
        "#3010 — a miss parks instead of failing closed synchronously"
    );

    let sends = sink.sends.lock().unwrap();
    let (identity, interest) = sends
        .iter()
        .find_map(|c| match c {
            ActorCommand::Interests(InterestsCommand::EnsureInterest { identity, interest }) => {
                Some((identity.clone(), interest.clone()))
            }
            _ => None,
        })
        .expect("expected an EnsureInterest for the recipient's kind:10019");
    assert_eq!(
        identity,
        crate::interests::recipient_nutzap_info_identity(RECIPIENT),
        "must dedupe against future lookups for the same recipient"
    );
    assert!(interest.shape.authors.contains(RECIPIENT));
    assert!(interest.shape.kinds.contains(&KIND_NIP61_NUTZAP_INFO));
}

/// #2936 — once the recipient's kind:10019 the interest above warmed has
/// actually arrived (simulated here by seeding `FixedCachedEvents`, the same
/// double a real kernel cache-serve would populate), a caller's own MANUAL
/// retry (a fresh dispatch under a new correlation id — independent of
/// #3010's auto-redrive continuation, which is exercised separately in
/// `send_nutzap_await_tests.rs`) finds it and proceeds PAST the
/// recipient-info gate — it must not fail with `NO_RECIPIENT_NUTZAP_INFO` a
/// second time.
#[test]
fn retry_after_interest_delivers_recipient_info_proceeds_past_the_gate() {
    let backend = backend_with_mint();
    let errors = run_send(&backend, &FixedCachedEvents::default(), "cid-retry-1", 21);
    assert_eq!(
        errors.last_token_code.lock().unwrap().as_deref(),
        None,
        "#3010 — the first miss parks instead of failing closed synchronously"
    );

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
    // Same backend (no proofs) — the next gate it should hit is insufficient
    // balance, never the recipient-info gate again.
    let errors = run_send(&backend, &cached, "cid-retry-2", 21);
    expect_error_code(&errors, ui_codes::INSUFFICIENT_BALANCE);
}

#[test]
fn recipient_info_with_no_relays_fails_closed() {
    let backend = backend_with_mint();
    let info = nmp_nip60::nutzap::NutZapInfo {
        relays: Vec::new(),
        mints: vec![MINT.to_string()],
        cashu_pubkey: Some("02".to_string() + &"44".repeat(32)),
    };
    let cached = FixedCachedEvents(vec![nutzap_info_kernel_event(
        RECIPIENT,
        &info,
        1_699_999_000,
    )]);
    let errors = run_send(&backend, &cached, "cid-no-relays", 21);
    expect_error_code(&errors, ui_codes::NO_RECIPIENT_RELAYS);
}

#[test]
fn recipient_info_with_no_p2pk_pubkey_fails_closed() {
    let backend = backend_with_mint();
    let info = nmp_nip60::nutzap::NutZapInfo {
        relays: vec!["wss://recipient-relay.example".to_string()],
        mints: vec![MINT.to_string()],
        cashu_pubkey: None,
    };
    let cached = FixedCachedEvents(vec![nutzap_info_kernel_event(
        RECIPIENT,
        &info,
        1_699_999_000,
    )]);
    let errors = run_send(&backend, &cached, "cid-no-p2pk", 21);
    expect_error_code(&errors, ui_codes::NO_RECIPIENT_P2PK);
}

#[test]
fn no_overlapping_mint_fails_closed() {
    let backend = backend_with_mint();
    let info = nmp_nip60::nutzap::NutZapInfo {
        relays: vec!["wss://recipient-relay.example".to_string()],
        mints: vec!["https://a-different-mint.example".to_string()],
        cashu_pubkey: Some("02".to_string() + &"44".repeat(32)),
    };
    let cached = FixedCachedEvents(vec![nutzap_info_kernel_event(
        RECIPIENT,
        &info,
        1_699_999_000,
    )]);
    let errors = run_send(&backend, &cached, "cid-no-mint", 21);
    expect_error_code(&errors, ui_codes::NO_TRUSTED_MINT);
}

#[test]
fn insufficient_balance_fails_closed() {
    let backend = backend_with_mint();
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
    // No proofs at all in `backend`'s state.
    let errors = run_send(&backend, &cached, "cid-insufficient", 21);
    expect_error_code(&errors, ui_codes::INSUFFICIENT_BALANCE);
}

/// #2997 (issue #2997): the recipient lists TWO mutually-trusted mints; this
/// wallet is only funded on the SECOND one. Before the fix, `run()` picked
/// the FIRST mutual mint unconditionally and failed the whole send with
/// `INSUFFICIENT_BALANCE` even though the second mutual mint could have
/// covered it. The corrected selection must skip the underfunded first mint
/// and settle on the funded second one.
///
/// Assertions are deliberately scoped to what `run()` does SYNCHRONOUSLY,
/// before it spawns the worker thread that would need a real mint HTTP
/// round-trip (no mock mint here, matching `insufficient_balance_fails_closed`'s
/// no-network style): (1) no fail-closed `UiToken` is raised through `ctx`
/// (the mint-choice gate's own failure path — worker-thread failures use a
/// different channel entirely, so this assertion cannot race the background
/// thread), and (2) the reserved proof was actually removed from MINT_B's
/// inventory (also done synchronously in `run()`, before the thread spawns),
/// proving the code proceeded to spend MINT_B rather than failing on MINT_A.
#[test]
fn send_selects_a_later_mutual_mint_when_the_first_is_underfunded() {
    const MINT_A: &str = "https://mint-a.example";
    const MINT_B: &str = "https://mint-b.example";

    let backend = CashuWalletBackend::new();
    {
        let mut state = state::lock_state(&backend.state);
        // This wallet accepts BOTH mints, but only holds proofs on MINT_B.
        state.mints = vec![MINT_A.to_string(), MINT_B.to_string()];
        state.add_proofs(None, MINT_B.to_string(), vec![synthetic_proof(100, "02bb")]);
    }
    let info = nmp_nip60::nutzap::NutZapInfo {
        relays: vec!["wss://recipient-relay.example".to_string()],
        // Recipient lists MINT_A first, then MINT_B — both mutual.
        mints: vec![MINT_A.to_string(), MINT_B.to_string()],
        cashu_pubkey: Some("02".to_string() + &"44".repeat(32)),
    };
    let cached = FixedCachedEvents(vec![nutzap_info_kernel_event(
        RECIPIENT,
        &info,
        1_699_999_000,
    )]);
    let errors = run_send(&backend, &cached, "cid-mutual-mint-fallback", 21);

    // No fail-closed token must have been raised: the send must have reached
    // past proof selection into the worker-thread mint round-trip, rather
    // than stopping at `NO_TRUSTED_MINT`/`INSUFFICIENT_BALANCE` on MINT_A.
    assert_eq!(
        errors.last_token_code.lock().unwrap().as_deref(),
        None,
        "a funded second mutual mint must not fail closed"
    );
    let state = state::lock_state(&backend.state);
    assert!(
        state.proofs.iter().all(|p| p.proof.c != "02bb"),
        "the reserved MINT_B proof must have been removed (spent), proving MINT_B was chosen"
    );
}

/// The core saga invariant, mirroring `deposit_tests`'s equivalent: consumed
/// inputs are journaled and the operation reaches `MintPending` synchronously
/// inside `start_intent`/`run` up to the point the mint swap would fire —
/// this test proves the ordering without any network access by stopping the
/// backend's own proof selection from ever finding a mint reachable (the
/// happy path is driven directly against the mint-HTTP-free `finish_send` in
/// `send_worker_tests.rs`).
#[test]
fn insufficient_balance_never_reaches_mint_pending() {
    let backend = backend_with_mint();
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
    let _rx = run_send(&backend, &cached, "cid-op-state", 21);
    let state = state::lock_state(&backend.state);
    let op = state
        .journal
        .get(&crate::journal::WalletOperationId::new("cid-op-state"))
        .unwrap();
    assert_eq!(
        op.state,
        crate::journal::WalletOperationState::Failed,
        "insufficient balance must fail closed before any mint HTTP call"
    );
}
