//! Standalone NIP-57 zap smoke test.
//!
//! Flow:
//!   1. Resolve recipient pubkey via NIP-05
//!   2. Generate ephemeral key for signing kind:9734
//!   3. Fetch bolt11 via LNURL-pay (using nmp-nip57)
//!   4. Build + sign NWC pay_invoice request (kind:23194)
//!   5. Connect to relay via WebSocket, subscribe + send
//!   6. Wait for kind:23195 payment confirmation
//!
//! Usage:
//!   cargo run -p zap-smoke -- [lightning-address] [sats] [comment]
//!
//! Defaults to: pablof7z@primal.net 1 "chirp-tui test"
//! NWC_URI env var overrides the hardcoded NWC connection string.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nostr::{EventBuilder, Keys, Kind, PublicKey, SecretKey, Tag, Timestamp};
use serde_json::{json, Value};
use tungstenite::Message;

const DEFAULT_NWC_URI: &str = "nostr+walletconnect://53e246c2e72bd8e0d12ffcb4c47776bf2eb785c9e5eb9027be858622f73b0703?relay=wss://relay.damus.io&relay=wss://relay.8333.space/&relay=wss://nos.lol&relay=wss://relay.primal.net&relay=wss://relay.primal.net &secret=e28f1ca1ff9ffd779eedef8523df2d473392cf8b47c8b4527c2fa73416c65472";
const DEFAULT_ADDRESS: &str = "pablof7z@primal.net";
const DEFAULT_SATS: u64 = 1;
const DEFAULT_COMMENT: &str = "chirp-tui test";
const NWC_TIMEOUT: Duration = Duration::from_secs(120);

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let address = args.get(1).map(String::as_str).unwrap_or(DEFAULT_ADDRESS);
    let sats: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_SATS);
    let comment = args.get(3).map(String::as_str).unwrap_or(DEFAULT_COMMENT);
    let nwc_uri = std::env::var("NWC_URI").unwrap_or_else(|_| DEFAULT_NWC_URI.to_string());

    println!("=== NWC Zap Smoke Test ===");
    println!("  address : {address}");
    println!("  amount  : {sats} sat");
    println!("  comment : {comment}");
    println!();

    if let Err(e) = run(&nwc_uri, address, sats, comment) {
        eprintln!("FAILED: {e}");
        std::process::exit(1);
    }
}

fn run(nwc_uri: &str, address: &str, sats: u64, comment: &str) -> Result<(), String> {
    let amount_msats = sats * 1000;

    // 1. NIP-05 pubkey resolution
    print!("[1/5] Resolving NIP-05 pubkey for {address} ... ");
    let recipient_pubkey = resolve_nip05(address)?;
    println!("{}", &recipient_pubkey[..16]);

    // 2. Parse NWC URI
    print!("[2/5] Parsing NWC URI ... ");
    let nwc = nmp_nwc::parse::NwcUri::parse(nwc_uri)
        .map_err(|e| format!("NWC URI parse error: {e}"))?;
    let client_sk = SecretKey::from_hex(nwc.client_secret_hex.as_str())
        .map_err(|e| format!("NWC client secret: {e}"))?;
    let client_keys = Keys::new(client_sk);
    let client_pubkey_hex = client_keys.public_key().to_hex();
    println!("wallet {}… | {} relays", &nwc.wallet_pubkey_hex[..16], nwc.relay_urls.len());

    // 3. Fetch bolt11 via LNURL-pay (NIP-57)
    print!("[3/5] Fetching bolt11 via LNURL-pay ... ");
    let zapper_keys = Keys::generate();
    let relays = vec![
        "wss://relay.damus.io".to_string(),
        "wss://relay.primal.net".to_string(),
    ];
    let bolt11 = nmp_nip57::fetch_bolt11_for_zap(
        &zapper_keys,
        address,
        amount_msats,
        &recipient_pubkey,
        &relays,
        Some(comment),
    )?;
    println!("{}…", &bolt11[..bolt11.len().min(40)]);

    // 4. Build + sign NWC pay_invoice request (kind:23194)
    print!("[4/5] Building NWC pay_invoice request ... ");
    let params = nmp_nwc::types::PayInvoiceParams {
        invoice: bolt11.clone(),
        amount: None,
    };
    let content = nmp_nwc::build::pay_invoice_content(
        nwc.client_secret_hex.as_str(),
        &nwc.wallet_pubkey_hex,
        &params,
    )
    .map_err(|e| format!("NWC encrypt: {e}"))?;

    let wallet_pk = PublicKey::from_hex(&nwc.wallet_pubkey_hex)
        .map_err(|e| format!("wallet pubkey: {e}"))?;
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let event = EventBuilder::new(Kind::from_u16(23194), &content)
        .tags([Tag::public_key(wallet_pk)])
        .custom_created_at(Timestamp::from(created_at))
        .sign_with_keys(&client_keys)
        .map_err(|e| format!("sign NWC request: {e}"))?;
    let request_id = event.id.to_hex();
    let event_json = serde_json::to_value(&event).map_err(|e| format!("serialize event: {e}"))?;
    println!("id {}…", &request_id[..16]);

    // 5. Connect to relay, subscribe to kind:23195, send EVENT
    println!("[5/5] Connecting to relay and sending payment ...");
    let relays_nwc = nwc.relay_urls.clone();
    let wallet_pubkey_hex = nwc.wallet_pubkey_hex.clone();
    let client_secret_str = nwc.client_secret_hex.as_str().to_string();

    let sub_id = format!("zap-smoke-{}", &request_id[..8]);
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_sub(5); // allow 5s of clock skew
    let req_msg = serde_json::to_string(&json!([
        "REQ",
        &sub_id,
        {
            "kinds": [23195u32],
            "authors": [&wallet_pubkey_hex],
            "#p": [&client_pubkey_hex],
            "since": since,
        }
    ]))
    .map_err(|e| format!("serialize REQ: {e}"))?;
    let event_msg = serde_json::to_string(&json!(["EVENT", event_json]))
        .map_err(|e| format!("serialize EVENT: {e}"))?;

    let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
    let request_id_clone = request_id.clone();

    // Try all NWC relays in parallel — the wallet might be connected to any of them.
    for relay in &relays_nwc {
        let tx2 = tx.clone();
        let relay2 = relay.clone();
        let req_msg2 = req_msg.clone();
        let event_msg2 = event_msg.clone();
        let wallet_pk2 = wallet_pubkey_hex.clone();
        let secret2 = client_secret_str.clone();
        let req_id2 = request_id_clone.clone();
        std::thread::spawn(move || {
            let result = nwc_roundtrip(&relay2, &req_msg2, &event_msg2, &wallet_pk2, &secret2, &req_id2);
            tx2.send(result).ok();
        });
    }
    drop(tx); // drop the original so rx closes when all threads finish

    // Collect results from all relay threads. Succeed on the first Ok; only
    // fail when all relay threads have returned errors.
    let deadline = Instant::now() + NWC_TIMEOUT;
    let mut last_err = String::from("all relays failed to connect");
    let mut relay_count = relays_nwc.len();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(format!("timed out after {}s waiting for NWC response", NWC_TIMEOUT.as_secs()));
        }
        match rx.recv_timeout(remaining) {
            Ok(Ok(preimage)) => {
                println!();
                println!("=== ZAP SENT ✓ ===");
                println!("  preimage: {preimage}");
                return Ok(());
            }
            Ok(Err(e)) => {
                // One relay failed — log and keep waiting for others.
                println!("  (relay error: {e})");
                last_err = e;
                relay_count -= 1;
                if relay_count == 0 {
                    return Err(format!("all relays failed: {last_err}"));
                }
            }
            Err(_) => {
                return Err(format!("timed out after {}s waiting for NWC response", NWC_TIMEOUT.as_secs()));
            }
        }
    }
}

fn nwc_roundtrip(
    relay_url: &str,
    req_msg: &str,
    event_msg: &str,
    wallet_pubkey_hex: &str,
    client_secret_hex: &str,
    expected_request_id: &str,
) -> Result<String, String> {
    let (mut ws, _) =
        tungstenite::connect(relay_url).map_err(|e| format!("WebSocket connect to {relay_url}: {e}"))?;

    ws.send(Message::Text(req_msg.to_string()))
        .map_err(|e| format!("send REQ: {e}"))?;
    ws.send(Message::Text(event_msg.to_string()))
        .map_err(|e| format!("send EVENT: {e}"))?;

    println!("  -> connected to {relay_url}");
    println!("  -> waiting for payment confirmation ...");

    let deadline = Instant::now() + Duration::from_secs(55);
    loop {
        if Instant::now() > deadline {
            return Err("deadline exceeded waiting for kind:23195".to_string());
        }
        match ws.read() {
            Ok(Message::Text(text)) => {
                println!("  <- {}", &text[..text.len().min(120)]);
                if let Some((req_id, response)) = nmp_nwc::decode::try_decode_response_for_request(
                    &text,
                    wallet_pubkey_hex,
                    client_secret_hex,
                ) {
                    if req_id != expected_request_id {
                        println!("  (response for different request {req_id}, ignoring)");
                        continue;
                    }
                    println!("  << matched response: result_type={:?} error={:?} result={:?}",
                        response.result_type, response.error, response.result);
                    if let Some(err) = response.error {
                        return Err(format!("{}: {}", err.code, err.message));
                    }
                    // Payment confirmed. Preimage is optional — some wallets omit it.
                    let preimage = response.pay_preimage()
                        .unwrap_or_else(|| "(wallet did not return preimage)".to_string());
                    return Ok(preimage);
                }
            }
            Ok(Message::Close(_)) => return Err("relay closed the connection".to_string()),
            Ok(_) => {}
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("timed out") || msg.contains("WouldBlock") {
                    continue;
                }
                return Err(format!("WebSocket read error: {msg}"));
            }
        }
    }
}

fn resolve_nip05(lightning_address: &str) -> Result<String, String> {
    let (user, domain) = lightning_address
        .split_once('@')
        .ok_or_else(|| format!("not a valid address: {lightning_address}"))?;
    let url = format!("https://{domain}/.well-known/nostr.json?name={user}");
    let resp: Value = ureq::get(&url)
        .timeout(Duration::from_secs(10))
        .call()
        .map_err(|e| format!("NIP-05 fetch {url}: {e}"))?
        .into_json()
        .map_err(|e| format!("NIP-05 JSON parse: {e}"))?;
    resp.get("names")
        .and_then(|n| n.get(user))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("NIP-05 pubkey not found for {user}@{domain}"))
}
