//! Shared fixtures for `cross_mint_headroom_fallback_tests.rs` — split out
//! purely for AGENTS.md's 500-LOC file-size discipline (the scripted local
//! TARGET/SOURCE mock mints below are the bulk of the size, not test logic).
//! Not shared with `cross_mint_headroom_tests.rs` (the standalone-action
//! twin) — that file's own `tests/` directory convention is "each submodule
//! defines its own small fixtures rather than sharing across siblings"; this
//! split is the one exception, purely because the fallback test and its own
//! mock-mint fixtures were originally one file too large for the cap.

use std::sync::{Arc, Mutex};

use nostr::secp256k1::{PublicKey, Secp256k1, SecretKey};

use super::*;

pub(super) const ACCOUNT: &str =
    "f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1";
pub(super) const RECIPIENT: &str =
    "e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2";
pub(super) const AMOUNT_SATS: u64 = 100;
/// Nonzero on purpose — a `0` fee would make this suite unable to tell
/// "headroom sized correctly" apart from "fee happens to be free".
const TARGET_FEE_PPK: u64 = 2000;
pub(super) const TARGET_QUOTE_ID: &str = "mint-quote-headroom-1";
const SOURCE_MELT_QUOTE_ID: &str = "melt-quote-headroom-1";

pub(super) fn ctx(account_pubkey: Option<&str>) -> WalletBackendContext<'_> {
    WalletBackendContext {
        now_secs: 1_700_000_000,
        selected_backend: None,
        account_pubkey,
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn dummy_pubkey() -> PublicKey {
    let sk = SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
    PublicKey::from_secret_key(&Secp256k1::new(), &sk)
}

/// A `/v1/keys` + `/v1/keysets` response body good for both endpoints (the
/// extra `input_fee_ppk` field `/v1/keys` doesn't need is simply ignored
/// there) — mirrors `cross_mint_resume_tests.rs`'s identical shortcut.
fn keyset_response_body(mint_pk: &PublicKey, input_fee_ppk: u64) -> String {
    let mut keys = serde_json::Map::new();
    for bit in 0..16u64 {
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
            "input_fee_ppk": input_fee_ppk,
        }]
    })
    .to_string()
}

/// Echo a blind signature (`C_ = B_`, the client's own blinded point) for
/// every blinded output in a `/v1/mint/bolt11` or `/v1/swap` request body.
/// This mock never verifies BDHKE math, and `DleqPolicy::VerifyIfPresent`
/// skips DLEQ entirely when no `dleq` field is sent — so an arbitrary
/// (never mint-signed) curve point still unblinds to a structurally valid
/// `Proof`, which is all downstream code in this test needs.
fn echo_signatures(body: &[u8]) -> String {
    let request: serde_json::Value =
        serde_json::from_slice(body).expect("valid blinded-outputs request");
    let signatures: Vec<serde_json::Value> = request["outputs"]
        .as_array()
        .expect("outputs array")
        .iter()
        .map(|out| {
            serde_json::json!({
                "amount": out["amount"],
                "id": out["id"],
                "C_": out["B_"],
            })
        })
        .collect();
    serde_json::json!({ "signatures": signatures }).to_string()
}

/// A scripted local TARGET mint serving the exact request sequence #3008's
/// fallback makes across three separate legs: the headroom-estimate keyset
/// fetch (1-2), the headroom-sized mint-quote create (3), the post-melt
/// status+keyset+mint-tokens leg (4-7), and finally the RE-DISPATCHED send's
/// own keyset+swap (8-10). `captured_funded_amount` records whatever
/// `amount` the mint-quote-create request actually asked for.
pub(super) fn spawn_target_mint(captured_funded_amount: Arc<Mutex<Option<u64>>>) -> String {
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind target mint");
    let addr = listener.local_addr().expect("local_addr");
    let url = format!("http://{addr}");
    std::thread::spawn(move || {
        let mint_pk = dummy_pubkey();
        let keys_body = keyset_response_body(&mint_pk, TARGET_FEE_PPK);

        // 1-2: the headroom estimate's own `get_sat_keyset` (keys, keysets).
        for _ in 0..2 {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = read_http_request(&mut stream);
            write_http_response(&mut stream, 200, &keys_body);
        }
        // 3: create_mint_quote(funded_amount_sats). The funded amount is
        // embedded in the (fake) invoice string so the SOURCE mock can echo
        // it back dynamically at melt-quote time — this test never
        // hardcodes the headroom constant on either side.
        {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let (buf, header_end) = read_http_request(&mut stream);
            let req: serde_json::Value =
                serde_json::from_slice(&buf[header_end..]).expect("valid mint-quote request");
            let amount = req["amount"].as_u64().expect("amount field");
            *captured_funded_amount.lock().unwrap() = Some(amount);
            write_http_response(
                &mut stream,
                200,
                &serde_json::json!({
                    "quote": TARGET_QUOTE_ID,
                    "request": format!("lnbc-fake-invoice-amount-{amount}"),
                    "amount": amount,
                    "unit": "sat",
                    "state": "UNPAID",
                    "expiry": null,
                })
                .to_string(),
            );
        }
        // 4: get_mint_quote_status — PAID (our own melt just paid it).
        {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = read_http_request(&mut stream);
            write_http_response(
                &mut stream,
                200,
                &serde_json::json!({
                    "quote": TARGET_QUOTE_ID,
                    "request": "lnbc-fake-invoice",
                    "amount": 0,
                    "unit": "sat",
                    "state": "PAID",
                    "expiry": null,
                })
                .to_string(),
            );
        }
        // 5-6: get_sat_keyset for mint_tokens.
        for _ in 0..2 {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = read_http_request(&mut stream);
            write_http_response(&mut stream, 200, &keys_body);
        }
        // 7: mint_tokens.
        {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let (buf, header_end) = read_http_request(&mut stream);
            let body = echo_signatures(&buf[header_end..]);
            write_http_response(&mut stream, 200, &body);
        }
        // 8-9: get_sat_keyset for the RETRIED send's own swap.
        for _ in 0..2 {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = read_http_request(&mut stream);
            write_http_response(&mut stream, 200, &keys_body);
        }
        // 10: the retried send's own swap.
        {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let (buf, header_end) = read_http_request(&mut stream);
            let body = echo_signatures(&buf[header_end..]);
            write_http_response(&mut stream, 200, &body);
        }
    });
    url
}

/// A scripted local SOURCE mint serving the melt leg: melt-quote create,
/// `get_sat_keyset`, then `melt` itself. `fee_reserve: 0` throughout — no
/// NUT-08 blank change outputs are ever offered or returned, so this mock
/// needs no BDHKE math for the melt leg at all.
pub(super) fn spawn_source_mint() -> String {
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind source mint");
    let addr = listener.local_addr().expect("local_addr");
    let url = format!("http://{addr}");
    std::thread::spawn(move || {
        let mint_pk = dummy_pubkey();
        let keys_body = keyset_response_body(&mint_pk, 0);

        // 1: create_melt_quote — dynamically echoes back whatever funded
        // amount TARGET's crafted invoice string carries.
        let funded_amount: u64 = {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let (buf, header_end) = read_http_request(&mut stream);
            let req: serde_json::Value =
                serde_json::from_slice(&buf[header_end..]).expect("valid melt-quote request");
            let bolt11 = req["request"].as_str().expect("request field").to_string();
            let amount: u64 = bolt11
                .rsplit('-')
                .next()
                .and_then(|s| s.parse().ok())
                .expect("invoice carries the funded-amount suffix");
            write_http_response(
                &mut stream,
                200,
                &serde_json::json!({
                    "quote": SOURCE_MELT_QUOTE_ID,
                    "request": bolt11,
                    "amount": amount,
                    "unit": "sat",
                    "fee_reserve": 0,
                    "state": "UNPAID",
                })
                .to_string(),
            );
            amount
        };
        // 2-3: get_sat_keyset.
        for _ in 0..2 {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = read_http_request(&mut stream);
            write_http_response(&mut stream, 200, &keys_body);
        }
        // 4: melt.
        {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = read_http_request(&mut stream);
            write_http_response(
                &mut stream,
                200,
                &serde_json::json!({
                    "quote": SOURCE_MELT_QUOTE_ID,
                    "request": "lnbc-fake-invoice",
                    "amount": funded_amount,
                    "unit": "sat",
                    "fee_reserve": 0,
                    "state": "PAID",
                    "change": [],
                })
                .to_string(),
            );
        }
    });
    url
}
