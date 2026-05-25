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

    // 3. Fetch bolt11 via LNURL-pay (NIP-57) — verbose path so we can inspect
    //    the kind:9734 and callback response.
    println!("[3/5] Fetching bolt11 via LNURL-pay (verbose) ...");
    let zapper_keys = Keys::generate();
    let zapper_pubkey = zapper_keys.public_key().to_hex();
    println!("  zapper pubkey : {zapper_pubkey}");
    let relays = vec![
        "wss://relay.damus.io".to_string(),
        "wss://relay.primal.net".to_string(),
    ];
    let (bolt11, _zap_request_json) = fetch_bolt11_verbose(
        &zapper_keys,
        address,
        amount_msats,
        &recipient_pubkey,
        &relays,
        Some(comment),
    )?;

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
                // Now wait and look for the kind:9735 receipt on the relays.
                println!();
                println!("[+] Checking for kind:9735 zap receipt ...");
                let recipient_pk = recipient_pubkey.clone();
                match wait_for_zap_receipt("wss://relay.primal.net", &recipient_pk, Duration::from_secs(90)) {
                    Ok(receipt_id) => println!("  kind:9735 receipt found! id={}", &receipt_id[..receipt_id.len().min(16)]),
                    Err(e) => println!("  WARNING: kind:9735 not seen on relay.primal.net within 90s: {e}"),
                }
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

/// Like `nmp_nip57::fetch_bolt11_for_zap` but prints the kind:9734 and full
/// callback response for debugging. Returns (bolt11, kind9734_json).
fn fetch_bolt11_verbose(
    keys: &nostr::Keys,
    lnurl_or_address: &str,
    amount_msats: u64,
    recipient_pubkey: &str,
    relays: &[String],
    comment: Option<&str>,
) -> Result<(String, String), String> {
    use nmp_nip57::build::ZapRequest;
    use nmp_nip57::{sign_zap_request, lnurl::{lnurl_to_well_known_url, url_encode_query}};
    use std::io::Read as _;

    // Build + sign kind:9734 — include the `lnurl` tag so the LNURL server
    // (Primal) can associate the payment with the right account and publish
    // the kind:9735 zap receipt (NIP-57 SHOULD).
    let mut builder = ZapRequest::to_pubkey(recipient_pubkey)
        .amount_msats(amount_msats)
        .relays(relays.to_vec())
        .lnurl(lnurl_or_address);
    if let Some(c) = comment {
        builder = builder.comment(c);
    }
    let pubkey_hex = keys.public_key().to_hex();
    let created_at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let unsigned = builder.build(pubkey_hex, created_at)
        .map_err(|e| format!("build kind:9734: {e}"))?;
    let signed_json = sign_zap_request(keys, &unsigned)?;

    println!("  kind:9734 event:");
    // Pretty-print the event JSON
    if let Ok(v) = serde_json::from_str::<Value>(&signed_json) {
        let pk = v["pubkey"].as_str().unwrap_or("");
        println!("    kind   : {}", v["kind"]);
        println!("    pubkey : {}…", &pk[..pk.len().min(16)]);
        println!("    tags   : {}", serde_json::to_string(&v["tags"]).unwrap_or_default());
        println!("    content: {:?}", v["content"]);
    }

    // Leg 1
    let well_known_url = lnurl_to_well_known_url(lnurl_or_address)?;
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(10)).build();
    let wk_resp: Value = agent.get(&well_known_url).call()
        .map_err(|e| format!("LNURL leg1 {well_known_url}: {e}"))?
        .into_json().map_err(|e| format!("LNURL leg1 parse: {e}"))?;
    println!("  LNURL metadata:");
    println!("    allowsNostr : {}", wk_resp["allowsNostr"]);
    println!("    nostrPubkey : {}", wk_resp["nostrPubkey"]);
    println!("    minSendable : {}", wk_resp["minSendable"]);
    println!("    maxSendable : {}", wk_resp["maxSendable"]);

    let callback = wk_resp["callback"].as_str()
        .ok_or("LNURL missing callback")?;
    let sep = if callback.contains('?') { '&' } else { '?' };
    let callback_url = format!("{callback}{sep}amount={amount_msats}&nostr={}",
        url_encode_query(&signed_json));
    println!("  callback url (first 120): {}…", &callback_url[..callback_url.len().min(120)]);

    // Leg 2
    let mut body = Vec::new();
    agent.get(&callback_url).call()
        .map_err(|e| format!("LNURL leg2: {e}"))?
        .into_reader()
        .take(65536)
        .read_to_end(&mut body)
        .map_err(|e| format!("LNURL leg2 read: {e}"))?;
    let callback_resp: Value = serde_json::from_slice(&body)
        .map_err(|e| format!("LNURL leg2 parse: {e}"))?;
    println!("  callback response: {}", serde_json::to_string(&callback_resp).unwrap_or_default());

    if let Some(status) = callback_resp["status"].as_str() {
        if status.eq_ignore_ascii_case("ERROR") {
            let reason = callback_resp["reason"].as_str().unwrap_or("unknown");
            return Err(format!("LNURL error: {reason}"));
        }
    }
    let bolt11 = callback_resp["pr"].as_str()
        .ok_or("LNURL callback missing pr (bolt11)")?.to_string();

    Ok((bolt11, signed_json))
}

/// Subscribe to kind:9735 zap receipts on `relay_url` filtered by the
/// recipient's pubkey (`#p` tag). Returns the event id on the first receipt
/// found; errors after `timeout` with no match.
///
/// Looks back 30 seconds (clock-skew buffer) in case the server minted the
/// receipt before we connected.
fn wait_for_zap_receipt(
    relay_url: &str,
    recipient_pubkey_hex: &str,
    timeout: Duration,
) -> Result<String, String> {
    let (mut ws, _) = tungstenite::connect(relay_url)
        .map_err(|e| format!("connect to {relay_url}: {e}"))?;

    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_sub(30);
    let req = serde_json::to_string(&json!([
        "REQ",
        "zap-receipt-watch",
        {
            "kinds": [9735u32],
            "#p": [recipient_pubkey_hex],
            "since": since,
        }
    ]))
    .map_err(|e| format!("serialize REQ: {e}"))?;
    ws.send(Message::Text(req)).map_err(|e| format!("send REQ: {e}"))?;

    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(format!("no kind:9735 seen within {}s", timeout.as_secs()));
        }
        match ws.read() {
            Ok(Message::Text(text)) => {
                let v: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if v.get(0).and_then(Value::as_str) == Some("EVENT") {
                    if let Some(event) = v.get(2) {
                        let kind = event.get("kind").and_then(Value::as_u64).unwrap_or(0);
                        if kind == 9735 {
                            let id = event
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            println!("  full receipt: {}",
                                serde_json::to_string(event).unwrap_or_default());
                            return Ok(id);
                        }
                    }
                }
            }
            Ok(Message::Close(_)) => return Err("relay closed connection".to_string()),
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
