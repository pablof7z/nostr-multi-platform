//! #3003 money-safety reconciliation: a `CrossMintTransfer` whose melt
//! already settled (source Lightning payment committed) but crashed before
//! the target-mint leg finished must, on cold-restart resume, finish
//! minting at the target WITHOUT ever re-attempting the melt (no
//! double-spend) and without double-minting (write-if-absent fence).
//!
//! Mirrors `deposit_mint_race_tests.rs`'s real BDHKE mint-side signing
//! pattern (a local mock mint that performs an actual NUT-00/NUT-04
//! blind-sign round trip, not a canned JSON body) so the resumed
//! `minted_proofs` really did come from a live `mint_tokens` call.

use super::*;
use nostr::secp256k1::{PublicKey, Scalar, Secp256k1, SecretKey};
use std::sync::Arc;

/// Minimal hex codec (no `hex` crate dependency in this crate) — test-only,
/// duplicated from `deposit_mint_race_tests.rs` (same reason: the real
/// codec lives in `nmp-nip60`, not reachable from this crate's tests).
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex"))
        .collect()
}

/// Real BDHKE mint-side signing: `C' = k*B'` for every blinded output the
/// mint-tokens request body carries, in the SAME order they arrived.
fn sign_mint_tokens_request(body: &[u8], mint_sk: &SecretKey) -> String {
    let secp = Secp256k1::new();
    let k_scalar = Scalar::from(*mint_sk);
    let request: serde_json::Value =
        serde_json::from_slice(body).expect("valid mint-tokens request body");
    let signatures: Vec<serde_json::Value> = request["outputs"]
        .as_array()
        .expect("outputs array")
        .iter()
        .map(|out| {
            let amount = out["amount"].as_u64().expect("output amount");
            let id = out["id"].as_str().expect("output keyset id").to_string();
            let b_prime_hex = out["B_"].as_str().expect("output B_");
            let b_prime = PublicKey::from_slice(&hex_decode(b_prime_hex)).expect("valid B'");
            let c_prime = b_prime.mul_tweak(&secp, &k_scalar).expect("k*B'");
            serde_json::json!({
                "amount": amount,
                "id": id,
                "C_": hex_encode(&c_prime.serialize()),
            })
        })
        .collect();
    serde_json::json!({ "signatures": signatures }).to_string()
}

fn keyset_response_body(mint_pk: &PublicKey) -> String {
    let mut keys = serde_json::Map::new();
    for bit in 0..8u64 {
        keys.insert(
            (1u64 << bit).to_string(),
            serde_json::Value::String(hex_encode(&mint_pk.serialize())),
        );
    }
    serde_json::json!({
        "keysets": [{
            "id": "00keyset",
            "unit": "sat",
            "keys": keys,
            "input_fee_ppk": 0,
        }]
    })
    .to_string()
}

/// A scripted local mock TARGET mint: mint-quote status (PAID) -> keys ->
/// keysets -> a REAL mint-tokens signature computed from whatever blinded
/// outputs the resumed worker actually sends.
fn spawn_target_mint(mint_sk: SecretKey, amount_sats: u64) -> String {
    use std::net::TcpListener;

    let mint_pk = PublicKey::from_secret_key(&Secp256k1::new(), &mint_sk);
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock target mint");
    let addr = listener.local_addr().expect("local_addr");
    let url = format!("http://{addr}");
    std::thread::spawn(move || {
        // 1. GET /v1/mint/quote/bolt11/{quote} — status check.
        if let Ok((mut stream, _)) = listener.accept() {
            let _ = read_http_request(&mut stream);
            write_http_response(
                &mut stream,
                200,
                &serde_json::json!({
                    "quote": "quote-target",
                    "request": "lnbc1testtarget",
                    "amount": amount_sats,
                    "unit": "sat",
                    "state": "PAID",
                    "expiry": null,
                })
                .to_string(),
            );
        }
        // 2. GET /v1/keys
        if let Ok((mut stream, _)) = listener.accept() {
            let _ = read_http_request(&mut stream);
            write_http_response(&mut stream, 200, &keyset_response_body(&mint_pk));
        }
        // 3. GET /v1/keysets (best-effort fee merge)
        if let Ok((mut stream, _)) = listener.accept() {
            let _ = read_http_request(&mut stream);
            write_http_response(
                &mut stream,
                200,
                &serde_json::json!({ "keysets": [] }).to_string(),
            );
        }
        // 4. POST /v1/mint/bolt11 — THE value-moving call, real BDHKE sign.
        if let Ok((mut stream, _)) = listener.accept() {
            let (buf, header_end) = read_http_request(&mut stream);
            let body = sign_mint_tokens_request(&buf[header_end..], &mint_sk);
            write_http_response(&mut stream, 200, &body);
        }
    });
    url
}

/// The exact #3003 crash-recovery scenario: melt already settled
/// (`melt_settled: true`, `minted_proofs: None`) when the process crashed.
/// `ResumeCrossMintTransferCommand` must mint at the target WITHOUT
/// re-attempting the melt (no source-mint listener is even spawned — if the
/// resume code mistakenly tried to melt again, the connection would fail
/// and this test's target-mint assertions below would never be satisfied)
/// and record the real, mint-issued proofs (never double-mint on a later
/// retry — the `minted_proofs.is_none()` write-if-absent fence).
#[test]
fn resume_after_melt_settled_finishes_mint_without_double_spend() {
    let backend = CashuWalletBackend::new();
    let operation_id = crate::journal::WalletOperationId::new("cid-cross-mint-resume");
    const AMOUNT_SATS: u64 = 20;

    let mint_sk = SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
    let target_mint_url = spawn_target_mint(mint_sk, AMOUNT_SATS);

    {
        let mut state = state::lock_state(&backend.state);
        state
            .begin_operation(operation_id.clone(), WalletOperationKind::CrossMintTransfer)
            .unwrap();
        // `CrossMintTransfer` requires a recorded consumed input before it
        // can reach `MintPending` (the pre-melt journal write every melt in
        // this codebase must make) — mirrors the real worker's own
        // `record_consumed_input` call for the melted source proofs.
        state
            .record_consumed_input(
                &operation_id,
                crate::journal::WalletConsumedInput {
                    event_id: String::new(),
                    mint: "https://source-mint.example".to_string(),
                    unit: "sat".to_string(),
                    amount: AMOUNT_SATS,
                },
            )
            .unwrap();
        state
            .transition(
                &operation_id,
                crate::journal::WalletOperationState::MintPending,
            )
            .unwrap();
        state
            .transition(
                &operation_id,
                crate::journal::WalletOperationState::MintSettled,
            )
            .unwrap();
        state.pending_cross_mint_transfers.insert(
            "quote-target".to_string(),
            state::PendingCrossMintTransfer {
                operation_id: operation_id.clone(),
                target_mint: target_mint_url.clone(),
                // Deliberately NOT a live listener — if the resume path
                // incorrectly tried to melt again, connecting here would
                // fail (proving the bug) rather than silently succeeding.
                source_mint: "http://127.0.0.1:1".to_string(),
                amount_sats: AMOUNT_SATS,
                target_quote_id: "quote-target".to_string(),
                melt_quote_id: "melt-quote-already-settled".to_string(),
                source_selected: Vec::new(),
                melt_settled: true,
                minted_proofs: None,
                signed_token: None,
                chain_started_at: None,
            },
        );
    }

    let cmd = Box::new(cross_mint_resume::ResumeCrossMintTransferCommand {
        state: Arc::clone(&backend.state),
        account_pubkey: "ee".repeat(32),
        target_quote_id: "quote-target".to_string(),
    });
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_000);
    let recipients = FixedRecipientLookup(Vec::new());
    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    {
        let mut c = ctx_with_sender(
            &send,
            nmp_core::CommandSender::new(worker_tx),
            &clock,
            &recipients,
        );
        cmd.run(&mut c).expect("run returns Ok");
    }

    // Proves the resumed worker actually reached the post-mint encrypt step
    // (the mint-tokens round trip against the target mock succeeded) rather
    // than erroring out or hanging trying to re-melt.
    match recv_command(&worker_rx) {
        nmp_core::actor::ActorCommand::Sign(
            nmp_core::actor::SignCommand::Nip44EncryptForAccount { .. },
        ) => {}
        other => panic!("expected the resume to reach the encrypt step, got {other:?}"),
    }

    let state = state::lock_state(&backend.state);
    let pending = state
        .pending_cross_mint_transfers
        .get("quote-target")
        .expect("pending cross-mint transfer retained");
    assert!(
        pending.melt_settled,
        "melt_settled must remain true — the resume never re-melted"
    );
    let minted = pending
        .minted_proofs
        .as_ref()
        .expect("resume must have minted real target-mint proofs");
    assert_eq!(
        minted.iter().map(|p| p.amount).sum::<u64>(),
        AMOUNT_SATS,
        "the resumed mint must issue exactly the transfer's amount"
    );
}
