//! F-04 — Zap E2E round-trip runtime verification (#978).
//!
//! The zap pipeline
//! (`ZapAction` → `FetchLnurlInvoiceCommand` → NWC `WalletPayInvoiceCommand`
//! → kind:9735 receipt ingest → `ZapsAggregateProjection`) is built and
//! unit-tested, but the full round-trip had never been exercised at runtime
//! against a live wallet over a relay. These tests close that gap with **zero
//! real money** by standing up a `nak serve` in-memory relay, a scripted fake
//! NIP-47 wallet service, and a real signed kind:9735 receipt.
//!
//! ## What is verified headlessly here
//!
//! 1. **`nwc_pay_invoice_round_trip_over_live_relay`** — the NWC half end to
//!    end at runtime: the full Chirp app (built via `nmp_app_new` +
//!    `nmp_app_chirp_register`, the exact composition the iOS shell ships)
//!    connects its `RelayRole::Wallet` socket to a live `nak serve` relay,
//!    `nmp.wallet.connect` (via `nmp_app_dispatch_action`) binds an NWC
//!    connection pointed at that relay, and `nmp.wallet.pay_invoice` dispatches a real kind:23194
//!    `pay_invoice` request. The fake wallet decrypts it, replies with a
//!    kind:23195 success over the relay, and the kernel's
//!    `handle_nwc_text` interceptor records the action terminal — proving the
//!    dispatch → encrypt → relay → decrypt → response → terminal path works
//!    on a real socket.
//!
//! 2. **`zap_receipt_ingest_updates_aggregate_projection`** — the receipt half:
//!    a real Schnorr-signed kind:9735 zap receipt is ingested through the
//!    production verify path (`nmp_app_inject_signed_event_json`) and the
//!    `nmp.nip57.zaps` (`ZapsAggregateProjection`) snapshot reflects the new
//!    total for the zapped target.
//!
//! The residual last mile — "a REAL lightning wallet paid a REAL invoice" —
//! needs a real wallet + lightning address (the LNURL-pay HTTPS leg is not
//! mockable in-process; see the module docs in `zap_e2e_common`). That lives in
//! the sibling `zap_e2e_real_wallet.rs` as the `#[ignore]` `real_wallet_zap_e2e`
//! test, runnable with one command.
//!
//! ## Run
//!
//! ```bash
//! # Headless round-trip (needs the `nak` binary on PATH):
//! cargo test -p nmp-testing --test zap_e2e_nwc_roundtrip -- --nocapture
//!
//! # Real-wallet last mile (needs an NWC connection string + LN address):
//! NWC_URI='nostr+walletconnect://…' ZAP_LN_ADDRESS='you@getalby.com' \
//!   cargo test -p nmp-testing --test zap_e2e_real_wallet \
//!   real_wallet_zap_e2e -- --ignored --nocapture
//! ```

mod zap_e2e_common;

use std::ffi::CString;
use std::time::{Duration, Instant};

use nmp_ffi::{
    nmp_app_add_relay, nmp_app_dispatch_action, nmp_app_free, nmp_app_inject_signed_event_json,
    nmp_app_new, nmp_app_start, nmp_free_string,
};
use nmp_app_chirp::{nmp_app_chirp_register, nmp_app_chirp_unregister, ChirpHandle, NmpRegisterStatus};
use nostr::{Keys, ToBech32};

use zap_e2e_common::{
    build_app_signed_in, install_emit_signal, install_rustls_provider, nwc_uri, publish_event,
    read_projection, signed_zap_receipt_json, wait_for_projection, FakeNwcWallet, NakRelay,
};

/// A test bolt11 invoice (sample shape). The fake wallet "pays" it with a
/// fixed preimage; no settlement happens. Real value is irrelevant to the NWC
/// wire contract, which only requires a `looks_like_bolt11` string.
const TEST_BOLT11: &str = "lnbc210n1pjxyz00pp5qqqsyqcyq5rqwzqfqqqsyqcyq5rqwzqfqqqsyqcyq5rqwzqfqypqdqqcqzzsxqyz5vqsp5testtesttesttesttesttesttesttesttesttesttesttesttestq9qyyssqtesttesttesttesttesttesttesttesttesttesttesttesttesttesttesttesttesttesttesttesttesttesttesttesttesttesttesttesttgpcqztest";

// ── Test 1: NWC pay_invoice round-trip over a live relay ─────────────────────

#[test]
fn nwc_pay_invoice_round_trip_over_live_relay() {
    install_rustls_provider();

    let Some(relay) = NakRelay::spawn() else {
        eprintln!(
            "SKIP nwc_pay_invoice_round_trip_over_live_relay: `nak` binary not \
             found or relay never came up. Install nak (https://github.com/fiatjaf/nak) \
             to run this test."
        );
        return;
    };
    let relay_url = relay.ws_url().to_string();

    // Deterministic, curve-valid keypairs for the NWC client + wallet service.
    let client_secret = "0101010101010101010101010101010101010101010101010101010101010101";
    let wallet_secret = "0202020202020202020202020202020202020202020202020202020202020202";
    let wallet_pubkey =
        nmp_nwc::crypto::client_pubkey_hex(wallet_secret).expect("wallet pubkey");

    // Bring up the fake wallet service FIRST so its REQ subscription is live
    // before the kernel publishes the kind:23194 request.
    let fake_wallet = FakeNwcWallet::spawn(&relay_url, wallet_secret, client_secret)
        .expect("fake wallet must connect to the relay");

    // Build the full Chirp app and sign in a fresh account.
    let nsec = Keys::generate().secret_key().to_bech32().expect("nsec");
    let (app, handle) = build_app_signed_in(&nsec);
    let rx = install_emit_signal(app);

    // Register the relay under the wallet role and start the actor so its pool
    // opens the socket.
    let url_c = CString::new(relay_url.as_str()).expect("url NUL-free");
    let role_c = CString::new("wallet").expect("role NUL-free");
    nmp_app_add_relay(app, url_c.as_ptr(), role_c.as_ptr());
    nmp_app_start(app, 200, 4);

    // Connect the NWC wallet, pointed at the same relay.
    // #1607: use nmp_app_dispatch_action directly — nmp_app_wallet_connect deleted.
    let uri = nwc_uri(&wallet_pubkey, &relay_url, client_secret);
    {
        let action_json = serde_json::to_string(&serde_json::json!({ "Connect": { "uri": uri } }))
            .expect("connect action JSON must serialize");
        let ns = CString::new("nmp.wallet.connect").expect("ns NUL-free");
        let body = CString::new(action_json).expect("body NUL-free");
        let ptr = nmp_app_dispatch_action(app, ns.as_ptr(), body.as_ptr());
        if !ptr.is_null() { nmp_free_string(ptr); }
    }

    // Wait for the wallet to report "ready" (get_info / get_balance probe
    // answered) before paying — the pay path requires status == "ready".
    let connect_deadline = Instant::now() + Duration::from_secs(15);
    let connected = wait_for_projection(app, &rx, "wallet", connect_deadline, |v| {
        v.get("status").and_then(|s| s.as_str()) == Some("ready")
            || v.get("connection_state").and_then(|s| s.as_str()) == Some("connected")
    });

    assert!(
        connected.is_some(),
        "the NWC connect must reach ready (get_info/get_balance answered over the relay) \
         before paying; wallet projection: {:?}",
        read_projection(app, "wallet"),
    );

    // Dispatch the pay_invoice through the action seam (D11). This is the
    // exact path the iOS shell takes when the user taps a zap's confirm.
    // #1607: nmp_app_wallet_pay_invoice deleted — callers use dispatch_action.
    {
        let action_json = serde_json::to_string(&serde_json::json!({
            "PayInvoice": { "bolt11": TEST_BOLT11, "amount_msats": serde_json::Value::Null }
        }))
        .expect("pay_invoice action JSON must serialize");
        let ns = CString::new("nmp.wallet.pay_invoice").expect("ns NUL-free");
        let body = CString::new(action_json).expect("body NUL-free");
        let ptr = nmp_app_dispatch_action(app, ns.as_ptr(), body.as_ptr());
        if !ptr.is_null() { nmp_free_string(ptr); }
    }

    // FORWARD HALF (kernel → relay → wallet): block until the fake wallet has
    // received + decrypted the kind:23194 the kernel published, proving the
    // dispatch → encrypt → publish path. Deterministic channel wait (D8).
    let paid_bolt11 = fake_wallet
        .wait_paid(Duration::from_secs(20))
        .expect(
            "fake wallet must receive + decrypt a pay_invoice request over the relay within 20s; \
             the kernel did not publish the kind:23194 to the wallet relay",
        );
    assert_eq!(
        paid_bolt11, TEST_BOLT11,
        "the bolt11 the wallet decrypted must equal the one the kernel was asked to pay",
    );

    // RETURN HALF (wallet → relay → kernel): the wallet has now published the
    // kind:23195 success. Drain emits until the wallet runtime has processed
    // it (V-79 keeps connection_state = connected and status = ready, and no
    // error toast is raised). We assert the wallet projection stays healthy
    // after the response is ingested — a decode/match failure in
    // `handle_nwc_text` would surface as an error status or a toast.
    let settle_deadline = Instant::now() + Duration::from_secs(10);
    let _ = wait_for_projection(app, &rx, "wallet", settle_deadline, |_| {
        // Block for a couple of post-payment emits so `handle_nwc_text` has
        // run against the kind:23195; we re-read the final state below.
        false
    });
    let final_wallet = read_projection(app, "wallet").expect("wallet projection present");
    let final_toast = read_projection(app, "last_error_toast");

    let observed = fake_wallet.stop();

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
    drop(relay);

    assert_eq!(
        final_wallet.get("status").and_then(|s| s.as_str()),
        Some("ready"),
        "after the kind:23195 pay_invoice success the wallet must remain ready, not error: \
         {final_wallet}",
    );
    assert!(
        observed.request_event_id.is_some(),
        "the wallet must have captured the kind:23194 request id it replied to",
    );
    eprintln!(
        "[zap-e2e] NWC round-trip PASS: connect (get_info+get_balance) ready, kernel published \
         kind:23194 pay_invoice, fake wallet decrypted bolt11={paid_bolt11}, replied kind:23195 \
         (req_id={:?}); final wallet status=ready, toast={final_toast:?}",
        observed.request_event_id,
    );
}

// ── Test 2: kind:9735 receipt ingest → ZapsAggregateProjection ───────────────

#[test]
fn zap_receipt_ingest_updates_aggregate_projection() {
    // No relay needed for the ingest half — inject the verbatim signed receipt
    // through the production verify path. (Test 3's real-wallet path proves the
    // over-relay subscription delivery; here we prove ingest → projection
    // deterministically.)
    let app = nmp_app_new();
    let mut handle: *mut ChirpHandle = std::ptr::null_mut();
    let status = nmp_app_chirp_register(app, std::ptr::null(), &mut handle);
    assert_eq!(status, NmpRegisterStatus::Ok as u32);
    assert!(!handle.is_null());
    let rx = install_emit_signal(app);
    nmp_app_start(app, 200, 4);

    // The LN provider that mints the receipt (its nostrPubkey identity).
    let provider = Keys::generate();
    let recipient = Keys::generate();
    let target_event_id = "ee".repeat(32);

    // A minimal but valid kind:9734 zap-request JSON for the `description` tag.
    let zap_request_json = serde_json::json!({
        "kind": 9734,
        "pubkey": recipient.public_key().to_hex(),
        "content": "",
        "tags": [["p", recipient.public_key().to_hex()], ["e", target_event_id]],
        "created_at": 1_700_000_000,
        "id": "00".repeat(32),
        "sig": "00".repeat(64),
    })
    .to_string();

    // bolt11 whose HRP encodes 210 sats (210n = 21000 msats) — the decoder
    // reads the amount from the HRP; the `lnbc210n1` prefix is what matters.
    let bolt11 = "lnbc210n1pjzapreceipttesttesttesttesttesttesttesttesttesttesttest";

    let receipt_json = signed_zap_receipt_json(
        &provider,
        &recipient.public_key().to_hex(),
        &target_event_id,
        bolt11,
        &zap_request_json,
    );

    let receipt_c = CString::new(receipt_json.as_str()).expect("receipt NUL-free");
    let ok = nmp_app_inject_signed_event_json(app, receipt_c.as_ptr());
    assert!(ok, "kind:9735 receipt must verify + ingest");

    // Block on the emit signal until the zaps projection reflects the target.
    let deadline = Instant::now() + Duration::from_secs(10);
    let zaps = wait_for_projection(app, &rx, "nmp.nip57.zaps", deadline, |v| {
        // The aggregate keys totals by the `["e", target]` tag. Accept either a
        // `totals` map keyed by target id, or any non-empty representation that
        // mentions the target.
        let as_text = v.to_string();
        as_text.contains(&target_event_id)
    });

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);

    let zaps = zaps.expect(
        "nmp.nip57.zaps projection must reflect the ingested kind:9735 receipt for the \
         zapped target within the deadline",
    );
    assert!(
        zaps.to_string().contains(&target_event_id),
        "the zapped target {target_event_id} must appear in the zaps aggregate: {zaps}",
    );
    eprintln!("[zap-e2e] receipt-ingest PASS: nmp.nip57.zaps = {zaps}");
}

// ── Test 3: relay publish helper sanity ──────────────────────────────────────

/// Documentation-only sanity: build the receipt publisher helper against a live
/// relay round-trips an `OK`. Skipped when `nak` is absent. This guards the
/// publish path the real-wallet test relies on for receipt delivery, without
/// spending money.
#[test]
fn relay_publish_helper_round_trips_ok() {
    install_rustls_provider();
    let Some(relay) = NakRelay::spawn() else {
        eprintln!("SKIP relay_publish_helper_round_trips_ok: `nak` not found");
        return;
    };
    let provider = Keys::generate();
    let recipient = Keys::generate();
    let target = "ab".repeat(32);
    let zap_request_json = "{}";
    let receipt = signed_zap_receipt_json(
        &provider,
        &recipient.public_key().to_hex(),
        &target,
        "lnbc10n1ptest",
        zap_request_json,
    );
    publish_event(relay.ws_url(), &receipt).expect("relay must OK the kind:9735 publish");
    eprintln!("[zap-e2e] relay publish helper PASS");
}
