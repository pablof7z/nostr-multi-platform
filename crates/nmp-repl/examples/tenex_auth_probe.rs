//! Probe `relay.tenex.chat` for kind:31933 events with NIP-42 auth.
//!
//! Demonstrates (and verifies) the auth race fix from issue #930:
//!
//!   1. Connect and send REQ immediately — relay closes it with auth-required.
//!   2. Relay sends AUTH challenge — we sign with a fresh ephemeral key.
//!   3. Relay responds OK (authenticated) — we re-send the REQ.
//!   4. Events arrive.
//!
//! This is the manual analogue of what the NMP kernel now does automatically
//! in `handle_auth_ok` via `lifecycle.handle_reconnect(relay_url)`.
//!
//! Run:
//!   cargo run -p nmp-repl --example tenex_auth_probe

use std::io::ErrorKind;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nostr::{EventBuilder, Keys, Kind, Tag};
use serde_json::{json, Value};
use tungstenite::Message;

use nmp_repl::ws::{try_connect_msg, Sock};

const RELAY: &str = "wss://relay.tenex.chat";
const SUB_ID: &str = "tenex-probe-1";
const WALL: Duration = Duration::from_secs(15);

// Read one frame as raw JSON, decoding the envelope tag.
enum Frame {
    Auth(String),
    OkAccepted(String),
    OkRejected(String, String),
    Closed(String, String),
    Event(Value),
    Eose,
    Other,
    Timeout,
    Gone,
}

fn read_frame(sock: &mut Sock) -> Frame {
    match sock.read() {
        Ok(Message::Text(t)) => parse(&t),
        Ok(Message::Close(_)) => Frame::Gone,
        Ok(_) => Frame::Other,
        Err(tungstenite::Error::Io(e))
            if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut =>
        {
            Frame::Timeout
        }
        Err(_) => Frame::Gone,
    }
}

fn parse(text: &str) -> Frame {
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return Frame::Other,
    };
    match v.get(0).and_then(Value::as_str) {
        Some("AUTH") => Frame::Auth(
            v.get(1)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        ),
        Some("OK") => {
            let id = v.get(1).and_then(Value::as_str).unwrap_or("").to_string();
            let ok = v.get(2).and_then(Value::as_bool).unwrap_or(false);
            let reason = v.get(3).and_then(Value::as_str).unwrap_or("").to_string();
            if ok {
                Frame::OkAccepted(id)
            } else {
                Frame::OkRejected(id, reason)
            }
        }
        Some("CLOSED") => Frame::Closed(
            v.get(1)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            v.get(2)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        ),
        Some("EVENT") => match v.get(2) {
            Some(ev) => Frame::Event(ev.clone()),
            None => Frame::Other,
        },
        Some("EOSE") => Frame::Eose,
        _ => Frame::Other,
    }
}

fn sign_auth(keys: &Keys, challenge: &str) -> (String, Value) {
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let ev = EventBuilder::new(Kind::from(22242), "")
        .custom_created_at(nostr::Timestamp::from_secs(created_at))
        .tag(Tag::parse(["relay", RELAY]).expect("relay tag"))
        .tag(Tag::parse(["challenge", challenge]).expect("challenge tag"))
        .sign_with_keys(keys)
        .expect("sign auth event");
    let id = ev.id.to_string();
    let wire = json!({
        "id": ev.id,
        "pubkey": ev.pubkey,
        "kind": ev.kind.as_u16(),
        "tags": ev.tags,
        "content": ev.content,
        "created_at": ev.created_at.as_u64(),
        "sig": ev.sig,
    });
    (id, wire)
}

fn main() {
    println!("=== tenex auth probe ===");
    println!("relay : {RELAY}");
    println!("filter: {{kinds:[31933]}}");
    println!();

    let keys = Keys::generate();
    println!("ephemeral pubkey: {}", keys.public_key());
    println!();

    println!("connecting…");
    let mut sock = match try_connect_msg(RELAY) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("connect failed: {e}");
            std::process::exit(1);
        }
    };
    println!("connected.\n");

    let req = json!(["REQ", SUB_ID, {"kinds": [31933]}]).to_string();

    // Send REQ immediately — before auth — to trigger the race.
    println!("→ REQ  kinds:[31933]  (before auth — expecting CLOSED auth-required)");
    sock.send(Message::Text(req.clone())).expect("send REQ");

    let mut event_count = 0u64;
    let mut pending_auth_id: Option<String> = None;
    let mut authenticated = false;
    let deadline = Instant::now() + WALL;

    while Instant::now() < deadline {
        match read_frame(&mut sock) {
            Frame::Auth(challenge) => {
                println!("← AUTH challenge: {challenge}");
                let (id, wire) = sign_auth(&keys, &challenge);
                pending_auth_id = Some(id.clone());
                println!("→ AUTH  event_id:{id}");
                sock.send(Message::Text(json!(["AUTH", wire]).to_string()))
                    .expect("send AUTH");
            }

            Frame::OkAccepted(id) => {
                if pending_auth_id.as_deref() == Some(&id) {
                    println!("← OK  ✓ authenticated  id:{id}");
                    authenticated = true;
                    pending_auth_id = None;
                    println!("→ REQ  kinds:[31933]  (re-subscribing after auth)");
                    sock.send(Message::Text(req.clone())).expect("resend REQ");
                }
            }

            Frame::OkRejected(id, reason) => {
                if pending_auth_id.as_deref() == Some(&id) {
                    eprintln!("← OK  ✗ auth rejected: {reason}");
                    std::process::exit(1);
                }
            }

            Frame::Closed(sub, reason) => {
                println!("← CLOSED  sub:{sub}  reason:{reason}");
                if reason.starts_with("auth-required") && !authenticated {
                    println!("  (expected — waiting for OK)");
                } else {
                    println!("  ✗ unexpected CLOSED");
                    break;
                }
            }

            Frame::Event(ev) => {
                event_count += 1;
                let id = ev.get("id").and_then(Value::as_str).unwrap_or("?");
                let kind = ev.get("kind").and_then(Value::as_u64).unwrap_or(0);
                let pubkey = ev
                    .get("pubkey")
                    .and_then(Value::as_str)
                    .map(|p| &p[..8])
                    .unwrap_or("?");
                println!("  ← EVENT #{event_count}  kind:{kind}  pubkey:{pubkey}…  id:{id}");
            }

            Frame::Eose => {
                println!("\n← EOSE  ({event_count} events received)");
                break;
            }

            Frame::Gone => {
                println!("connection closed by relay");
                break;
            }

            Frame::Timeout | Frame::Other => {}
        }
    }

    println!("\n=== result ===");
    println!("authenticated : {authenticated}");
    println!("events        : {event_count} kind:31933");
    if authenticated && event_count > 0 {
        println!("✓ NIP-42 auth works; relay returns kind:31933 events after re-subscribe");
    } else if !authenticated {
        println!("✗ auth did not complete");
    } else {
        println!("✗ no events after auth");
    }

    let _ = sock.close(None);
}
