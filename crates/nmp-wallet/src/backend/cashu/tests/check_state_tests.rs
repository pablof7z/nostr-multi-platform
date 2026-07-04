//! `check_state::run_check_state_pass` (#2977) — the NUT-07 reconciliation
//! pass exercised against a local mock mint (`tests/mod.rs`'s
//! `spawn_mock_mint`): a recovered wallet whose mint reports some proofs
//! spent must end up with exactly the unspent ones left in `state.proofs`,
//! reflected in the ledger balance, and still selectable by
//! `select_proofs` — never the reverse (a mint HTTP failure must leave
//! every proof for that mint untouched).

use crate::backend::cashu::ingest::ingest_token_event;
use crate::backend::cashu::tests::spawn_mock_mint;
use crate::backend::cashu::CashuWalletBackend;

use super::*;

/// The `Y = hash_to_curve(secret)` hex `MintClient::check_state` derives
/// internally from the request — a mock mint response must echo exactly
/// this back, in the same order as the secrets were sent, or
/// `parse_check_state_response` rejects it (see `checkstate.rs`).
fn y_hex(secret: &str) -> String {
    hex::encode(
        nmp_nip60::cashu::crypto::hash_to_curve(secret.as_bytes())
            .expect("hash_to_curve")
            .serialize(),
    )
}

fn proof(amount: u64, c: &str, secret: &str) -> nmp_nip60::cashu::types::Proof {
    nmp_nip60::cashu::types::Proof {
        amount,
        id: "keyset-1".to_string(),
        secret: secret.to_string(),
        c: c.to_string(),
        dleq: None,
        witness: None,
    }
}

fn token_event_json(mint: &str, proofs: &[nmp_nip60::cashu::types::Proof]) -> String {
    serde_json::json!({ "mint": mint, "proofs": proofs, "del": Vec::<String>::new() }).to_string()
}

/// Recover 3 proofs from one mint; the mint reports 2 spent + 1 unspent.
/// After the pass: only the unspent proof remains in `state.proofs`, the
/// ledger balance reflects only it, and `select_proofs` can still find it —
/// exactly the acceptance scenario #2977 exists to close (a spent proof
/// `select_proofs` would otherwise keep re-offering to `nutzap.send`, which
/// would fail every time at the mint's own swap call).
#[test]
fn drops_only_the_proofs_the_mint_affirmatively_reports_spent() {
    let a = proof(10, "c-a", "secret-a");
    let b = proof(5, "c-b", "secret-b");
    let c = proof(7, "c-c", "secret-c");

    let body = serde_json::json!({
        "states": [
            {"Y": y_hex("secret-a"), "state": "SPENT"},
            {"Y": y_hex("secret-b"), "state": "SPENT"},
            {"Y": y_hex("secret-c"), "state": "UNSPENT"},
        ]
    })
    .to_string();
    let mock_mint = spawn_mock_mint(vec![(200, body)]);

    let backend = CashuWalletBackend::new();
    let plaintext = token_event_json(&mock_mint, &[a, b, c]);
    ingest_token_event(&backend.state, "token-1", &plaintext, "").expect("ingest must succeed");
    assert_eq!(lock_state(&backend.state).proofs.len(), 3, "all 3 proofs loaded before the pass");

    run_check_state_pass(&backend.state);

    let state = lock_state(&backend.state);
    assert_eq!(state.proofs.len(), 1, "the 2 spent proofs must be dropped");
    assert_eq!(state.proofs[0].proof.c, "c-c");
    assert_eq!(
        state.ledger.state().balance(
            &crate::journal::MintUrl::new(canonicalize_mint_url(&mock_mint)),
            &crate::journal::WalletUnit::new("sat")
        ),
        7_000,
        "ledger balance must reflect only the unspent proof"
    );
    let (selected, total) = state
        .select_proofs(&mock_mint, 7)
        .expect("the unspent proof must still be selectable");
    assert_eq!(total, 7);
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].proof.c, "c-c");
}

/// All proofs unspent — a no-op pass, nothing removed.
#[test]
fn leaves_proofs_untouched_when_the_mint_reports_all_unspent() {
    let a = proof(10, "c-a", "secret-a");
    let b = proof(5, "c-b", "secret-b");

    let body = serde_json::json!({
        "states": [
            {"Y": y_hex("secret-a"), "state": "UNSPENT"},
            {"Y": y_hex("secret-b"), "state": "UNSPENT"},
        ]
    })
    .to_string();
    let mock_mint = spawn_mock_mint(vec![(200, body)]);

    let backend = CashuWalletBackend::new();
    let plaintext = token_event_json(&mock_mint, &[a, b]);
    ingest_token_event(&backend.state, "token-1", &plaintext, "").expect("ingest must succeed");

    run_check_state_pass(&backend.state);

    assert_eq!(lock_state(&backend.state).proofs.len(), 2);
}

/// Fail-safe: a mint that errors (a 500, or an unparsable body) must never
/// cause its proofs to be dropped — only an AFFIRMATIVE `SPENT` verdict
/// removes a proof. The existing swap-time fail-safe remains the backstop
/// for anything this pass could not reach.
#[test]
fn a_mint_http_failure_never_drops_proofs_for_that_mint() {
    let a = proof(10, "c-a", "secret-a");
    let mock_mint = spawn_mock_mint(vec![(500, "{\"code\":1,\"detail\":\"boom\"}".to_string())]);

    let backend = CashuWalletBackend::new();
    let plaintext = token_event_json(&mock_mint, &[a]);
    ingest_token_event(&backend.state, "token-1", &plaintext, "").expect("ingest must succeed");

    run_check_state_pass(&backend.state);

    assert_eq!(
        lock_state(&backend.state).proofs.len(),
        1,
        "a check-state HTTP failure must never drop a proof it couldn't reconcile"
    );
}

/// No held proofs at all (nothing recovered yet) — a fast, HTTP-free no-op
/// rather than a panic or an empty-mint request.
#[test]
fn no_held_proofs_is_a_no_op() {
    let backend = CashuWalletBackend::new();
    run_check_state_pass(&backend.state);
    assert!(lock_state(&backend.state).proofs.is_empty());
}

/// Two distinct mints — only the spent proof at the mint that reports it
/// spent is dropped; the other mint's proof (never even reached, since it
/// is a separate mock server nothing here dials) is left alone. This isn't
/// asserted via a second live mock (the pass only ever calls a mint that
/// actually holds proofs), but confirms the grouping doesn't cross-
/// contaminate the two mints' verdicts.
#[test]
fn distinct_mints_are_checked_and_folded_independently() {
    let spent_at_mint_a = proof(10, "c-a", "secret-a");
    let mint_a = spawn_mock_mint(vec![(
        200,
        serde_json::json!({ "states": [{"Y": y_hex("secret-a"), "state": "SPENT"}] }).to_string(),
    )]);
    let unspent_at_mint_b = proof(3, "c-b", "secret-b");
    let mint_b = spawn_mock_mint(vec![(
        200,
        serde_json::json!({ "states": [{"Y": y_hex("secret-b"), "state": "UNSPENT"}] })
            .to_string(),
    )]);

    let backend = CashuWalletBackend::new();
    ingest_token_event(
        &backend.state,
        "token-a",
        &token_event_json(&mint_a, &[spent_at_mint_a]),
        "",
    )
    .expect("ingest a must succeed");
    ingest_token_event(
        &backend.state,
        "token-b",
        &token_event_json(&mint_b, &[unspent_at_mint_b]),
        "",
    )
    .expect("ingest b must succeed");

    run_check_state_pass(&backend.state);

    let state = lock_state(&backend.state);
    assert_eq!(state.proofs.len(), 1);
    assert_eq!(state.proofs[0].proof.c, "c-b");
}
