//! LIVE NIP-AD integration test (#2927) — network-gated behind `#[ignore]`.
//!
//! Proves the whole NIP-AD path end to end against a real endpoint:
//!   1. `resolve_ad_url_blocking("https://trellis.rs/legible")` fetches
//!      `https://trellis.rs/.well-known/nostr.json?ad=%2Flegible`, selects the
//!      `/legible` entry, and yields its `{filter, relays}`.
//!   2. We connect to the returned relays, run the resolved filter, and assert
//!      at least one `kind:30023` event comes back whose `d` tag matches.
//!
//! Offline in CI (ignored by default). Run explicitly:
//!   NMP_AD_LIVE=1 cargo test -p nmp-nip-ad --test live_trellis -- --ignored --nocapture

use std::io::ErrorKind;
use std::net::TcpStream;
use std::time::{Duration, Instant};

use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

const AD_URL: &str = "https://trellis.rs/legible";
const EXPECT_KIND: u64 = 30023;
const EXPECT_AUTHOR: &str = "3f68dede81549cc0844fafe528f1574b51e095e7491f468bd9689f87779bb81d";
const EXPECT_D: &str = "the-machine-that-could-tell-you-why";

#[test]
#[ignore = "live network — run with `NMP_AD_LIVE=1 cargo test -p nmp-nip-ad --test live_trellis -- --ignored`"]
fn trellis_legible_resolves_and_returns_kind_30023() {
    // ── Step 1: resolve the AD URL through the real .well-known endpoint. ──
    let res = nmp_nip_ad::resolve_ad_url_blocking(AD_URL)
        .unwrap_or_else(|e| panic!("resolve_ad_url_blocking({AD_URL}) failed: {e}"));

    let filter_json = serde_json::to_value(&res.filter).expect("filter serializes");
    println!("── resolved AD entry for {AD_URL} ──");
    println!("filter = {filter_json}");
    println!("relays = {:?}", res.relays);

    assert_eq!(
        filter_json["kinds"],
        serde_json::json!([EXPECT_KIND]),
        "resolved filter must target kind 30023"
    );
    assert_eq!(
        filter_json["authors"],
        serde_json::json!([EXPECT_AUTHOR]),
        "resolved filter must carry the expected author"
    );
    assert_eq!(
        filter_json["#d"],
        serde_json::json!([EXPECT_D]),
        "resolved filter must carry the expected #d"
    );
    assert!(
        res.relays.iter().any(|r| r == "wss://relay.primal.net"),
        "resolved relays must include wss://relay.primal.net, got {:?}",
        res.relays
    );

    // ── Step 2: run the filter against the site-supplied relays. ──
    let req = serde_json::json!(["REQ", "nip-ad-live", filter_json]).to_string();
    let mut matched: Option<serde_json::Value> = None;

    for relay in &res.relays {
        println!("── connecting {relay} ──");
        match query_relay(relay, &req) {
            Ok(Some(event)) => {
                println!("← matched kind:30023 event from {relay}");
                matched = Some(event);
                break;
            }
            Ok(None) => println!("  (no matching event before EOSE/timeout)"),
            Err(e) => println!("  relay error (continuing): {e}"),
        }
    }

    let event = matched.expect(
        "at least one relay must return a kind:30023 event whose d tag matches the resolved filter",
    );
    // Final assertions on the actual event pulled off the wire.
    assert_eq!(event["kind"].as_u64(), Some(EXPECT_KIND), "event kind");
    let d_tag = d_tag_of(&event).expect("event must have a d tag");
    assert_eq!(d_tag, EXPECT_D, "event d tag matches");
    println!(
        "✔ live proof: {AD_URL} resolved to kind:30023 event id={} d={d_tag}",
        event["id"].as_str().unwrap_or("?")
    );
}

/// Connect to one relay, run `req`, and return the first EVENT whose kind and
/// `d` tag match. `Ok(None)` on EOSE/timeout without a match.
fn query_relay(relay: &str, req: &str) -> Result<Option<serde_json::Value>, String> {
    let (mut socket, response) =
        tungstenite::connect(relay).map_err(|e| format!("connect: {e}"))?;
    println!("  HTTP {} — connected", response.status());
    set_read_timeout(&mut socket, Duration::from_millis(500));

    socket
        .send(Message::Text(req.to_string()))
        .map_err(|e| format!("send REQ: {e}"))?;

    let deadline = Instant::now() + Duration::from_secs(12);
    while Instant::now() < deadline {
        match socket.read() {
            Ok(Message::Text(text)) => {
                let frame: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                match frame.get(0).and_then(serde_json::Value::as_str) {
                    Some("EVENT") => {
                        if let Some(event) = frame.get(2) {
                            if event_matches(event) {
                                let _ = socket.close(None);
                                return Ok(Some(event.clone()));
                            }
                        }
                    }
                    Some("EOSE") => {
                        // Stored events exhausted with no match.
                        let _ = socket.close(None);
                        return Ok(None);
                    }
                    _ => {}
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {}
            Err(e) => return Err(format!("read: {e}")),
        }
    }
    let _ = socket.close(None);
    Ok(None)
}

fn event_matches(event: &serde_json::Value) -> bool {
    event["kind"].as_u64() == Some(EXPECT_KIND) && d_tag_of(event) == Some(EXPECT_D)
}

/// The value of the first `d` tag on an event JSON, if any.
fn d_tag_of(event: &serde_json::Value) -> Option<&str> {
    event["tags"].as_array()?.iter().find_map(|t| {
        let arr = t.as_array()?;
        if arr.first()?.as_str()? == "d" {
            arr.get(1)?.as_str()
        } else {
            None
        }
    })
}

fn set_read_timeout(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>, timeout: Duration) {
    let _ = match socket.get_ref() {
        MaybeTlsStream::Plain(s) => s.set_read_timeout(Some(timeout)),
        MaybeTlsStream::Rustls(s) => s.get_ref().set_read_timeout(Some(timeout)),
        _ => Ok(()),
    };
}
