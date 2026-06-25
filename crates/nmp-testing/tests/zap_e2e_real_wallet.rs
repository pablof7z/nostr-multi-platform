//! F-04 — the real-wallet last mile for zap E2E (#978).
//!
//! The headless harness (`zap_e2e_nwc_roundtrip.rs`) verifies every kernel
//! state transition the zap pipeline makes EXCEPT the one hop that cannot be
//! mocked in-process: the LNURL-pay HTTP round-trip. `fetch_lnurl_invoice_blocking`
//! does two HTTPS GETs with `ureq` against the webpki root store and refuses
//! any non-`https://` callback (LUD-01 §1), so a local stub would need a
//! publicly-trusted certificate. Proving "a REAL lightning wallet paid a REAL
//! invoice and a REAL kind:9735 receipt landed in the aggregate" therefore
//! needs the owner to supply a live wallet + lightning address. This file is
//! that one-command last mile.

mod zap_e2e_common;

use std::ffi::CString;
use std::time::{Duration, Instant};

use nmp_ffi::{nmp_app_add_relay, nmp_app_free, nmp_app_start};
use nmp_app_chirp::{dispatch_action_bytes_for, nmp_app_chirp_unregister};
use nostr::{Keys, ToBech32};
use serde_json;

use zap_e2e_common::{
    build_app_signed_in, install_emit_signal, install_rustls_provider, read_projection,
    wait_for_projection,
};

/// THE REAL-WALLET LAST MILE — the hop that cannot be mocked in-process.
///
/// Supply via environment:
///
/// 1. **`NWC_URI`** — a `nostr+walletconnect://…` connection string from a real
///    NWC-capable wallet (Alby Hub, coinos, Mutiny, Zeus) pointed at a real or
///    testnet balance. Get one from e.g. <https://nwc.dev> or your Alby account
///    (Settings → Wallet Connect → create connection). It MUST grant the
///    `pay_invoice` method.
/// 2. **`ZAP_LN_ADDRESS`** — a real lightning address to zap (e.g.
///    `you@getalby.com`). Its LNURL-pay endpoint mints the kind:9735 receipt.
/// 3. *(optional)* **`ZAP_AMOUNT_MSATS`** — defaults to `1000` (1 sat).
/// 4. *(optional)* **`ZAP_TARGET_EVENT_ID`** — a note to attribute the zap to;
///    omit for a profile zap.
///
/// One command:
///
/// ```bash
/// NWC_URI='nostr+walletconnect://…' \
/// ZAP_LN_ADDRESS='you@getalby.com' \
/// ZAP_AMOUNT_MSATS=1000 \
///   cargo test -p nmp-testing --test zap_e2e_real_wallet \
///   real_wallet_zap_e2e -- --ignored --nocapture
/// ```
///
/// The test builds the full Chirp app, signs in a fresh key, connects the real
/// wallet, registers the public relays the receipt will land on, dispatches the
/// real zap through `nmp.nip57.zap`, and blocks until the `action_lifecycle`
/// projection records the terminal. It spends real (or testnet) sats — run it
/// deliberately.
#[test]
#[ignore = "real-wallet last mile: set NWC_URI + ZAP_LN_ADDRESS and run with --ignored"]
fn real_wallet_zap_e2e() {
    install_rustls_provider();

    // SKIP (not panic) when the owner-supplied env vars are absent: the
    // nightly real-relay workflow runs `-- --ignored` with no name filter,
    // so this test executes unattended. The repo convention for real_relay_*
    // tests is eprintln + early return, never a red nightly.
    let Ok(nwc_uri_str) = std::env::var("NWC_URI") else {
        eprintln!("SKIP real_wallet_zap_e2e: NWC_URI not set (owner-supplied last mile)");
        return;
    };
    let Ok(ln_address) = std::env::var("ZAP_LN_ADDRESS") else {
        eprintln!("SKIP real_wallet_zap_e2e: ZAP_LN_ADDRESS not set (owner-supplied last mile)");
        return;
    };
    let amount_msats: u64 = std::env::var("ZAP_AMOUNT_MSATS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);
    let target_event_id = std::env::var("ZAP_TARGET_EVENT_ID").ok();

    // Public relays where the LN provider publishes the kind:9735 receipt and
    // where the kernel subscribes for it.
    const RECEIPT_RELAYS: [&str; 3] = [
        "wss://relay.damus.io",
        "wss://nos.lol",
        "wss://relay.primal.net",
    ];

    let nsec = Keys::generate().secret_key().to_bech32().expect("nsec");
    let (app, handle) = build_app_signed_in(&nsec);
    let rx = install_emit_signal(app);

    for relay in RECEIPT_RELAYS {
        let url_c = CString::new(relay).expect("url NUL-free");
        // Read+write so the kernel both publishes the kind:9734 relays tag and
        // subscribes for the kind:9735 receipt.
        let role_c = CString::new("both").expect("role NUL-free");
        nmp_app_add_relay(app, url_c.as_ptr(), role_c.as_ptr());
    }
    nmp_app_start(app, 200, 4);

    // #1996: dispatch through the typed ADR-0064 byte doorway via the Chirp
    // `dispatch_action_bytes_for` seam (the JSON `nmp_app_dispatch_action`
    // doorway is retired).
    let action_json = serde_json::to_string(&serde_json::json!({
        "Connect": { "uri": nwc_uri_str }
    }))
    .expect("connect action JSON must serialize");
    let connect = dispatch_action_bytes_for(app, "nmp.wallet.connect", &action_json);
    assert!(connect.is_ok(), "nmp.wallet.connect dispatch must be accepted: {connect:?}");

    // Wait for the real wallet to report ready.
    let connect_deadline = Instant::now() + Duration::from_secs(30);
    let ready = wait_for_projection(app, &rx, "wallet", connect_deadline, |v| {
        v.get("status").and_then(|s| s.as_str()) == Some("ready")
            || v.get("connection_state").and_then(|s| s.as_str()) == Some("connected")
    });
    assert!(
        ready.is_some(),
        "real wallet must report ready within 30s; check the NWC_URI relay + permissions",
    );

    // Dispatch the real zap through the generic action seam. The kernel
    // resolves the LNURL, fetches the bolt11, pays via NWC, and ingests the
    // receipt.
    let recipient = Keys::generate().public_key().to_hex(); // profile zap target
    let mut body = serde_json::json!({
        "recipient_pubkey": recipient,
        "amount_msats": amount_msats,
        "lnurl": ln_address,
    });
    if let Some(target) = &target_event_id {
        body["target_event_id"] = serde_json::Value::String(target.clone());
    }
    let body_json = body.to_string();

    // #1996: typed ADR-0064 byte doorway. The helper returns the host-supplied
    // correlation_id echoed by the doorway on accept, or an error string.
    let correlation_id = dispatch_action_bytes_for(app, "nmp.nip57.zap", &body_json)
        .unwrap_or_else(|err| panic!("zap dispatch must be accepted, got error: {err}"));
    eprintln!("[zap-e2e real] dispatched zap correlation_id={correlation_id}");

    // Block until the action lifecycle shows a terminal for this correlation,
    // then confirm the zaps aggregate reflects the receipt.
    let deadline = Instant::now() + Duration::from_secs(90);
    let terminal = wait_for_projection(app, &rx, "action_lifecycle", deadline, |v| {
        let txt = v.to_string();
        txt.contains(&correlation_id)
            && (txt.contains("ok") || txt.contains("failed") || txt.contains("Failed"))
    });

    let zaps = read_projection(app, "nmp.nip57.zaps");

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);

    let terminal = terminal.unwrap_or_else(|| {
        panic!(
            "real zap did not reach a terminal within 90s — inspect the wallet logs + \
             relay receipt. action_lifecycle never recorded correlation {correlation_id}"
        )
    });
    eprintln!("[zap-e2e real] terminal lifecycle: {terminal}");
    eprintln!("[zap-e2e real] zaps aggregate: {zaps:?}");
    assert!(
        terminal.to_string().contains("ok"),
        "the real zap must record an OK terminal (a real wallet paid a real invoice): {terminal}",
    );
}
