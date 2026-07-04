//! #3008 — `nutzap.send`'s cross-mint auto-fallback must be SELF-SUFFICIENT:
//! the transfer it triggers has to fund the target mint with enough headroom
//! to cover the re-dispatched send's OWN P2PK swap fee there, or that retry
//! underflows `INSUFFICIENT_BALANCE` before ever reaching the mint (the
//! target mint holds EXACTLY `amount_sats`, so `gross_change` is `0` and any
//! nonzero fee fails the `gross_change < fee` guard in `send_worker.rs`).
//!
//! This file owns the STANDALONE-action half of that proof (the explicit
//! `nmp.wallet.cashu.cross_mint_transfer` action must transfer EXACTLY
//! `amount_sats`, never headroom) — the full melt -> mint -> publish ->
//! retry chain proving the FALLBACK actually settles lives in
//! `cross_mint_headroom_fallback_tests.rs` (AGENTS.md file-size discipline;
//! that chain needs two scripted local mock mints and doesn't fit here
//! alongside this test under the 500-LOC hard cap).

use std::sync::{Arc, Mutex};

use super::*;

const AMOUNT_SATS: u64 = 100;

fn ctx(account_pubkey: Option<&str>) -> WalletBackendContext<'_> {
    WalletBackendContext {
        now_secs: 1_700_000_000,
        selected_backend: None,
        account_pubkey,
    }
}

/// The standalone action must transfer EXACTLY `amount_sats` — the target
/// mint's very FIRST request (its mint-quote create) must carry that exact
/// amount, never more. A deliberately-broken response short-circuits the
/// rest of the saga (this test only cares about what was ASKED FOR, not
/// whether the transfer completes) — if the fallback's headroom logic ever
/// leaked into this path, the captured amount would be too large.
#[test]
fn standalone_cross_mint_transfer_funds_exactly_the_requested_amount() {
    let captured_amount: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(None));
    let capture_for_thread = Arc::clone(&captured_amount);
    let target_mint_url = {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind target mint");
        let addr = listener.local_addr().expect("local_addr");
        let url = format!("http://{addr}");
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let (buf, header_end) = read_http_request(&mut stream);
            let req: serde_json::Value =
                serde_json::from_slice(&buf[header_end..]).unwrap_or(serde_json::Value::Null);
            *capture_for_thread.lock().unwrap() = req.get("amount").and_then(|v| v.as_u64());
            write_http_response(&mut stream, 400, r#"{"code":1,"detail":"stop-here"}"#);
        });
        url
    };
    let source_mint_url = "https://source-mint-standalone.example".to_string();

    let backend = CashuWalletBackend::new();
    {
        let mut state = state::lock_state(&backend.state);
        state.add_proofs(
            None,
            source_mint_url,
            vec![synthetic_proof(
                1_000,
                &("02".to_string() + &"cc".repeat(32)),
            )],
        );
    }
    let account = "aa".repeat(32);
    let commands = backend.start_intent(
        ctx(Some(&account)),
        WalletIntent::CrossMintTransfer {
            target_mint: target_mint_url,
            amount_sats: AMOUNT_SATS,
        },
        Some("cid-standalone".to_string()),
    );
    let ActorCommand::Protocol(cmd) = commands.into_iter().next().unwrap() else {
        panic!("expected a Protocol command");
    };

    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
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
    // Drain the (intentionally-triggered) failure the broken mock response
    // produces — blocks until the mock has genuinely received and captured
    // the request, so the assertion below is never a race.
    let _ = recv_command(&worker_rx);

    assert_eq!(
        captured_amount
            .lock()
            .unwrap()
            .expect("amount must have been captured"),
        AMOUNT_SATS,
        "the standalone action must fund EXACTLY amount_sats — no headroom"
    );
}
