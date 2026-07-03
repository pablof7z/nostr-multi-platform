//! #2946 money-safety regression: a `CompleteDepositCashu` attempt whose real
//! NUT-04 `mint_tokens` response is slow enough to outlive
//! `DEPOSIT_CHAIN_LEASE_SECS` must still have its real, already-minted
//! proofs recorded into `minted_proofs`, even though a retry took over the
//! `chain_started_at` lease for the same `quote_id` while the slow response
//! was still in flight. #2941 accidentally fenced that persist on
//! `chain_started_at == Some(created_at)` (mirroring `on_signed`'s fold
//! fence), which drops the winning attempt's real proofs on exactly this
//! race — stranding real sats at balance zero with no recorded proofs to
//! recover from. See `complete.rs`'s persist-step doc comment for why
//! write-if-absent (not fenced on the lease) is the correct guard: NUT-04
//! single-issue means only one attempt for a `quote_id` ever reaches this
//! point with real proofs at all.
//!
//! Runs a real `CashuCompleteDepositCommand` against a local mock mint that
//! performs an actual NUT-00/NUT-04 blind-sign round trip (not a canned
//! JSON body) so the winning attempt's `minted_proofs` really did come from
//! `mint_tokens`, and pauses right before the mint-tokens response is
//! written back — the rendezvous point this test uses to interleave a
//! same-quote lease takeover exactly like a real concurrent retry would.

use super::*;
use nmp_core::actor::SignCommand;
use nostr::secp256k1::{PublicKey, Scalar, Secp256k1, SecretKey};
use std::sync::{mpsc, Arc};

/// A response a [`spawn_scripted_mock_mint`] connection serves.
enum ScriptedResponse {
    /// A canned status/body, written back as soon as the request is read.
    Fixed(u16, String),
    /// The NUT-04 mint-tokens response: computed for real from whatever
    /// blinded outputs the request actually carries (`C' = k*B'`, `mint_sk`
    /// signing every denomination — mirrors `nmp_nip60`'s own
    /// `mint_http_support::fixture_keyset` test fixture, duplicated here
    /// since that helper is `pub(crate)` to the `nmp-nip60` crate and not
    /// reachable from this one). Signals `ready` once the request is fully
    /// read and the response is about to be withheld, then blocks on
    /// `release` before writing it — the pause a test uses to interleave a
    /// state mutation between "the mint received the real request" and "the
    /// client sees the response".
    SignMintTokens {
        mint_sk: SecretKey,
        ready: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    },
}

/// Minimal hex codec (no `hex` crate dependency in this crate) — test-only.
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
/// request body carries, in the SAME order they arrived (the client matches
/// signatures to outputs positionally — see `finalize_blinded_outputs`).
fn sign_mint_tokens_request(body: &[u8], mint_sk: &SecretKey) -> serde_json::Value {
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
    serde_json::json!({ "signatures": signatures })
}

/// A `/v1/keys` response body for a single keyset (id `"00keyset"`) whose
/// per-denomination pubkey (bits 0..8, i.e. amounts 1..=128) is the SAME
/// `mint_pk` for every denomination — enough to mint any amount up to 255.
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

/// Like `mod.rs`'s `spawn_mock_mint`, but driven by [`ScriptedResponse`]
/// instead of plain `(status, body)` pairs so the last connection (the
/// mint-tokens POST) can compute a real signature from the request it
/// actually received and pause before responding. Reuses `mod.rs`'s
/// `read_http_request`/`write_http_response` for the actual HTTP framing.
fn spawn_scripted_mock_mint(responses: Vec<ScriptedResponse>) -> String {
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock mint listener");
    let addr = listener.local_addr().expect("local_addr");
    let url = format!("http://{addr}");
    std::thread::spawn(move || {
        for response in responses {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let (buf, header_end) = read_http_request(&mut stream);
            let (status, body) = match response {
                ScriptedResponse::Fixed(status, body) => (status, body),
                ScriptedResponse::SignMintTokens {
                    mint_sk,
                    ready,
                    release,
                } => {
                    let signatures = sign_mint_tokens_request(&buf[header_end..], &mint_sk);
                    let _ = ready.send(());
                    let _ = release.recv();
                    (200, signatures.to_string())
                }
            };
            write_http_response(&mut stream, status, &body);
        }
    });
    url
}

/// The exact #2946 race: attempt A's `mint_tokens` response is held back
/// past a simulated lease takeover for the SAME `quote_id`, then released —
/// A must still persist the real proofs it actually received, not drop them
/// because a newer attempt's `chain_started_at` no longer matches its own.
#[test]
fn a_slow_mint_response_outlasting_a_lease_takeover_still_persists_its_real_proofs() {
    let backend = backend_with_mint();
    let operation_id = crate::journal::WalletOperationId::new("cid-race");
    {
        let mut state = state::lock_state(&backend.state);
        state
            .begin_operation(operation_id.clone(), WalletOperationKind::DepositCashu)
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
        state.pending_deposits.insert(
            "quote-race".to_string(),
            state::PendingDeposit {
                operation_id: operation_id.clone(),
                mint: MINT.to_string(),
                amount_sats: 15,
                minted_proofs: None,
                signed_token: None,
                chain_started_at: None,
            },
        );
    }

    let secp = Secp256k1::new();
    let mint_sk = SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
    let mint_pk = PublicKey::from_secret_key(&secp, &mint_sk);

    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let mock_url = spawn_scripted_mock_mint(vec![
        ScriptedResponse::Fixed(
            200,
            serde_json::json!({
                "quote": "quote-race",
                "request": "lnbc150n1testnut",
                "amount": 15,
                "unit": "sat",
                "state": "PAID",
                "expiry": null,
            })
            .to_string(),
        ),
        // GET /v1/keys
        ScriptedResponse::Fixed(200, keyset_response_body(&mint_pk)),
        // GET /v1/keysets (best-effort fee merge — see `get_sat_keyset`).
        ScriptedResponse::Fixed(200, serde_json::json!({ "keysets": [] }).to_string()),
        // POST /v1/mint/bolt11 — THE value-moving call. Paused.
        ScriptedResponse::SignMintTokens {
            mint_sk,
            ready: ready_tx,
            release: release_rx,
        },
    ]);

    let cmd = Box::new(deposit::CashuCompleteDepositCommand {
        state: Arc::clone(&backend.state),
        operation_id: operation_id.clone(),
        quote_id: "quote-race".to_string(),
        mint: mock_url,
        amount_sats: 15,
        account_pubkey: "ee".repeat(32),
        correlation_id: Some("cid-race".to_string()),
    });
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    // Attempt A's own lease starts at t=0.
    let clock = FixedClock(0);
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

    // Block until attempt A's mint-tokens request has actually reached the
    // mint and its response is being withheld — mirrors a real mint whose
    // response takes longer than `DEPOSIT_CHAIN_LEASE_SECS`.
    ready_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("mint-tokens request reached the mock mint");

    // A retry takes over the lease for the SAME `quote_id` while A is still
    // waiting on the mint's response — exactly what a real retry's own
    // resume block does once it observes an expired `chain_started_at` (see
    // `PendingDeposit::chain_started_at`'s doc comment).
    {
        let mut state = state::lock_state(&backend.state);
        state
            .pending_deposits
            .get_mut("quote-race")
            .unwrap()
            .chain_started_at = Some(999);
    }

    // Let the mint's real response through — attempt A now reaches its own
    // persist step holding real, non-empty proofs, with a `chain_started_at`
    // that no longer matches its own `created_at` (0).
    release_tx.send(()).unwrap();

    // A proceeds into `dispatch_token_event`'s encrypt step — proof it
    // actually reached the post-mint code path rather than erroring out.
    match recv_command(&worker_rx) {
        ActorCommand::Sign(SignCommand::Nip44EncryptForAccount { .. }) => {}
        other => panic!("expected attempt A to proceed into the encrypt step, got {other:?}"),
    }

    let state = state::lock_state(&backend.state);
    let pending = state
        .pending_deposits
        .get("quote-race")
        .expect("pending deposit retained");
    assert!(
        pending.minted_proofs.is_some(),
        "attempt A actually minted real proofs from the mint before B's takeover was even \
         possible in real time — they must be persisted for balance/recovery regardless of \
         `chain_started_at` no longer matching; dropping them here strands real sats (#2946)"
    );
    assert_eq!(
        pending
            .minted_proofs
            .as_ref()
            .unwrap()
            .iter()
            .map(|p| p.amount)
            .sum::<u64>(),
        15,
        "the persisted proofs must be the ones the mint actually issued"
    );
    // B's takeover lease is untouched by A's persist — only the
    // `minted_proofs` gate changed, not the lease semantics `on_signed` (a
    // separate fence, unrelated to this fix) still relies on.
    assert_eq!(
        state
            .pending_deposits
            .get("quote-race")
            .unwrap()
            .chain_started_at,
        Some(999)
    );
}
