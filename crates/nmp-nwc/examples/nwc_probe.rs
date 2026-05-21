//! NIP-47 NWC end-to-end probe.
//!
//! Connects to each relay in a NWC URI in turn (or a single relay via the
//! second arg). For each: opens the WebSocket, handles NIP-42 AUTH with the
//! client secret, subscribes for kind:23195, sends `get_info` + `get_balance`,
//! prints every wire frame, decodes kind:23195 responses.
//!
//! Use this to isolate NWC protocol bugs from kernel / host-app plumbing.
//!
//! Run:
//! ```
//! cargo run --example nwc_probe -p nmp-nwc -- 'nostr+walletconnect://...'
//! cargo run --example nwc_probe -p nmp-nwc -- 'nostr+walletconnect://...' wss://nos.lol
//! ```

use std::env;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nostr::{EventBuilder, Keys, Kind, PublicKey, SecretKey, Tag, TagKind, Timestamp};
use nmp_nwc::build;
use nmp_nwc::decode::try_decode_relay_message_with_id;
use nmp_nwc::parse::NwcUri;
use nmp_nwc::NwcMethod;
use serde_json::json;
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

type Sock = WebSocket<MaybeTlsStream<TcpStream>>;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn main() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let uri = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: nwc_probe <nostr+walletconnect://...> [relay_url]");
        std::process::exit(2);
    });

    let parsed = NwcUri::parse(&uri).unwrap_or_else(|e| {
        eprintln!("parse error: {e}");
        std::process::exit(1);
    });

    println!("== parsed ==");
    println!("  wallet_pubkey = {}", parsed.wallet_pubkey_hex);
    println!("  client_secret = {}…", &parsed.client_secret_hex.as_str()[..8]);
    println!("  relays        = {:?}", parsed.relay_urls);

    let client_pubkey_hex = nmp_nwc::crypto::client_pubkey_hex(parsed.client_secret_hex.as_str())
        .expect("derive client pubkey");
    println!("  client_pubkey = {}", client_pubkey_hex);

    let override_relay = env::args().nth(2);
    let relays: Vec<String> = if let Some(r) = override_relay {
        vec![r]
    } else {
        parsed.relay_urls.clone()
    };

    for relay in &relays {
        println!();
        println!("████ probing {relay} ████");
        match probe_one(relay, &parsed, &client_pubkey_hex) {
            Ok(true) => println!("  ✓ got a wallet response"),
            Ok(false) => println!("  ✗ no wallet response within timeout"),
            Err(e) => println!("  ✗ error: {e}"),
        }
    }
}

fn probe_one(
    relay_url: &str,
    parsed: &NwcUri,
    client_pubkey_hex: &str,
) -> Result<bool, String> {
    let (mut socket, response) = tungstenite::connect(relay_url)
        .map_err(|e| format!("connect: {e}"))?;
    println!("  HTTP {} — connected", response.status());

    // Polling read timeout.
    let _ = match socket.get_ref() {
        MaybeTlsStream::Plain(s) => s.set_read_timeout(Some(Duration::from_millis(500))),
        MaybeTlsStream::Rustls(s) => s.get_ref().set_read_timeout(Some(Duration::from_millis(500))),
        _ => Ok(()),
    };

    let client_sk = SecretKey::from_hex(parsed.client_secret_hex.as_str())
        .map_err(|e| format!("client secret: {e}"))?;
    let client_keys = Keys::new(client_sk);
    let wallet_pk = PublicKey::from_hex(&parsed.wallet_pubkey_hex)
        .map_err(|e| format!("wallet pubkey: {e}"))?;
    let sub_id = format!("nwc-{}", &parsed.wallet_pubkey_hex[..8]);

    // 1. REQ subscription for kind:23195.
    let req_filter = json!({
        "kinds": [23195u32],
        "authors": [&parsed.wallet_pubkey_hex],
        "#p": [client_pubkey_hex],
    });
    let req_msg = json!(["REQ", &sub_id, &req_filter]).to_string();
    println!("→ REQ {sub_id}");
    socket.send(Message::Text(req_msg)).map_err(|e| e.to_string())?;

    // 2. get_info + get_balance EVENTs.
    for method in [NwcMethod::GetInfo, NwcMethod::GetBalance] {
        let content = build::request_content(
            parsed.client_secret_hex.as_str(),
            &parsed.wallet_pubkey_hex,
            &method,
            json!({}),
        )
        .map_err(|e| format!("encrypt: {e}"))?;
        let event = EventBuilder::new(Kind::from_u16(23194), &content)
            .tags([Tag::public_key(wallet_pk)])
            .custom_created_at(Timestamp::from(now_secs()))
            .sign_with_keys(&client_keys)
            .map_err(|e| format!("sign 23194: {e}"))?;
        let event_json = json!({
            "id": event.id.to_hex(),
            "pubkey": event.pubkey.to_hex(),
            "created_at": event.created_at.as_secs(),
            "kind": 23194,
            "tags": event.tags.iter().map(|t| t.as_slice().to_vec()).collect::<Vec<_>>(),
            "content": event.content,
            "sig": event.sig.to_string(),
        });
        let text = json!(["EVENT", event_json]).to_string();
        println!("→ {method:?} EVENT id={}", &event.id.to_hex()[..16]);
        socket.send(Message::Text(text)).map_err(|e| e.to_string())?;
    }

    // 3. Pump frames until we get a kind:23195 response or timeout.
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut got_response = false;
    while Instant::now() < deadline {
        match socket.read() {
            Ok(Message::Text(s)) => {
                if handle_text(&s, parsed, &client_keys, relay_url, &mut socket)? {
                    got_response = true;
                    break;
                }
            }
            Ok(Message::Close(frame)) => {
                println!("← CLOSE {frame:?}");
                break;
            }
            Ok(other) => {
                println!("← {other:?}");
            }
            Err(tungstenite::Error::Io(e))
                if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut =>
            {
                // poll timeout, keep going
            }
            Err(e) => return Err(format!("read: {e}")),
        }
    }
    let _ = socket.close(None);
    Ok(got_response)
}

/// Returns `Ok(true)` if a kind:23195 response was decoded.
fn handle_text(
    text: &str,
    parsed: &NwcUri,
    client_keys: &Keys,
    relay_url: &str,
    socket: &mut Sock,
) -> Result<bool, String> {
    let preview = if text.len() > 200 { &text[..200] } else { text };
    println!("← {preview}{}", if text.len() > 200 { "…" } else { "" });

    if text.starts_with("[\"AUTH\"") {
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let arr = value.as_array().ok_or("AUTH not array")?;
        let challenge = arr
            .get(1)
            .and_then(|v| v.as_str())
            .ok_or("AUTH missing challenge")?
            .to_string();
        let relay_tag = Tag::custom(
            TagKind::Custom(std::borrow::Cow::Borrowed("relay")),
            [relay_url],
        );
        let challenge_tag = Tag::custom(
            TagKind::Custom(std::borrow::Cow::Borrowed("challenge")),
            [challenge.as_str()],
        );
        let event = EventBuilder::new(Kind::from_u16(22242), "")
            .tags([relay_tag, challenge_tag])
            .custom_created_at(Timestamp::from(now_secs()))
            .sign_with_keys(client_keys)
            .map_err(|e| format!("sign 22242: {e}"))?;
        let wire = json!([
            "AUTH",
            {
                "id": event.id.to_hex(),
                "pubkey": event.pubkey.to_hex(),
                "kind": 22242u32,
                "tags": event.tags.iter().map(|t| t.as_slice().to_vec()).collect::<Vec<_>>(),
                "content": event.content,
                "created_at": event.created_at.as_secs(),
                "sig": event.sig.to_string(),
            }
        ])
        .to_string();
        println!("→ AUTH response id={}", &event.id.to_hex()[..16]);
        socket.send(Message::Text(wire)).map_err(|e| e.to_string())?;
        return Ok(false);
    }

    if let Some((event_id, response)) =
        try_decode_relay_message_with_id(
            text,
            &parsed.wallet_pubkey_hex,
            parsed.client_secret_hex.as_str(),
        )
    {
        println!(
            "  decoded NWC event_id={} result_type={} error={:?}",
            &event_id[..16.min(event_id.len())],
            response.result_type,
            response.error
        );
        if let Some(bal) = response.balance_msats() {
            println!("  *** BALANCE = {} msats ({} sats) ***", bal, bal / 1000);
            return Ok(true);
        }
        if response.result_type == "get_info" && response.error.is_none() {
            println!("  *** get_info OK ***");
            return Ok(true);
        }
    }
    Ok(false)
}
