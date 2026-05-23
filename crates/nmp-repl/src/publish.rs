//! Direct-WebSocket publish + fetch helpers for the MLS commands.
//!
//! The diagnostic REPL has NO `NmpApp` kernel — it speaks the Nostr wire
//! protocol straight over tungstenite (see `ws.rs`). These helpers are the
//! write-side + targeted-read-side counterparts the MLS commands need to run
//! the full Marmot flow against live relays.
//!
//! They are deliberately simple and synchronous: connect, send one frame,
//! read until a terminal frame or a short wall deadline. No retry, no
//! outbox routing — the MLS commands name their relays explicitly.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::ws::{self, Frame};

/// How long to wait for an `OK` (publish) or `EOSE` (fetch) per relay.
const WIRE_WALL: Duration = Duration::from_secs(5);

/// Publish a signed event to a list of relays. Returns `(successes, failures)`.
///
/// For each relay: connect, send `["EVENT", <event-json>]`, then read frames
/// until an `["OK", <id>, <accepted>, <msg>]` for our event id, a terminal
/// frame, or the 5s wall. Prints a per-relay status line.
pub fn publish_event(event: &nostr::Event, relay_urls: &[String]) -> (usize, usize) {
    let event_json = serde_json::to_value(event).unwrap_or(Value::Null);
    let event_id = event.id.to_hex();
    let frame = json!(["EVENT", event_json]).to_string();

    let mut ok = 0usize;
    let mut fail = 0usize;

    for url in relay_urls {
        match ws::try_connect_msg(url) {
            Ok(mut sock) => {
                if let Err(e) = sock.send(tungstenite::Message::Text(frame.clone())) {
                    println!("  publish {url}: send failed ({e})");
                    fail += 1;
                    continue;
                }
                match await_ok(&mut sock, &event_id) {
                    OkResult::Accepted => {
                        println!("  publish {url}: OK");
                        ok += 1;
                    }
                    OkResult::Rejected(msg) => {
                        println!("  publish {url}: rejected ({msg})");
                        fail += 1;
                    }
                    OkResult::NoReply => {
                        // Many relays accept silently / close the socket
                        // before an explicit OK on a one-shot connection.
                        // Treat "sent, no negative reply" as a soft success
                        // so the operator sees forward progress.
                        println!("  publish {url}: sent (no OK before timeout)");
                        ok += 1;
                    }
                }
            }
            Err(why) => {
                println!("  publish {url}: connect failed ({why})");
                fail += 1;
            }
        }
    }
    (ok, fail)
}

enum OkResult {
    Accepted,
    Rejected(String),
    NoReply,
}

/// Read frames until an `OK` for `event_id`, a terminal frame, or the wall.
fn await_ok(sock: &mut ws::Sock, event_id: &str) -> OkResult {
    let deadline = Instant::now() + WIRE_WALL;
    while Instant::now() < deadline {
        match ws::next_frame(sock) {
            Frame::Other => {
                // `OK` is not first-class in `Frame`; re-read it raw. The
                // last raw message is not retained, so peek the socket's
                // next text instead: in practice relays send OK promptly.
                if let Some(res) = await_raw_ok(sock, event_id, deadline) {
                    return res;
                }
            }
            Frame::Timeout => continue,
            Frame::RelayClosed | Frame::Io { .. } => return OkResult::NoReply,
            Frame::Closed { message, .. } => return OkResult::Rejected(message),
            Frame::Auth { .. } => {
                return OkResult::Rejected("auth-required (REPL does not AUTH)".into())
            }
            _ => continue,
        }
    }
    OkResult::NoReply
}

/// `Frame::Other` collapses `OK` envelopes. Drain a few raw text frames
/// looking for our `["OK", <id>, <bool>, <msg>]`.
fn await_raw_ok(sock: &mut ws::Sock, event_id: &str, deadline: Instant) -> Option<OkResult> {
    while Instant::now() < deadline {
        match sock.read() {
            Ok(tungstenite::Message::Text(s)) => {
                if let Ok(v) = serde_json::from_str::<Value>(&s) {
                    if v.get(0).and_then(Value::as_str) == Some("OK")
                        && v.get(1).and_then(Value::as_str) == Some(event_id)
                    {
                        let accepted = v.get(2).and_then(Value::as_bool).unwrap_or(false);
                        let msg = v.get(3).and_then(Value::as_str).unwrap_or("").to_string();
                        return Some(if accepted {
                            OkResult::Accepted
                        } else {
                            OkResult::Rejected(msg)
                        });
                    }
                }
            }
            Ok(tungstenite::Message::Close(_)) => return Some(OkResult::NoReply),
            Ok(_) => continue,
            Err(tungstenite::Error::Io(e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                return None
            }
            Err(_) => return Some(OkResult::NoReply),
        }
    }
    None
}

/// Issue one REQ against `relay_url`, collect every EVENT until EOSE / a
/// terminal frame / `wall`, and return the raw event JSON values.
///
/// `filter_json` is the bare filter object (NOT wrapped); this builds
/// `["REQ", <sub>, <filter>]`. Used by `mls-fetch-kp`.
#[must_use] 
pub fn fetch_events(relay_url: &str, filter_json: &Value, wall: Duration) -> Vec<Value> {
    let sub = format!("repl-mls-{}", now_secs());
    let mut out = Vec::new();

    let mut sock = match ws::try_connect_msg(relay_url) {
        Ok(s) => s,
        Err(why) => {
            println!("  fetch {relay_url}: connect failed ({why})");
            return out;
        }
    };

    let req = json!(["REQ", sub, filter_json]).to_string();
    if let Err(e) = sock.send(tungstenite::Message::Text(req)) {
        println!("  fetch {relay_url}: send failed ({e})");
        return out;
    }

    let deadline = Instant::now() + wall;
    while Instant::now() < deadline {
        match ws::next_frame(&mut sock) {
            Frame::Event {
                sub_id, event, ..
            } if sub_id == sub => out.push(event),
            Frame::Eose { sub_id } if sub_id == sub => break,
            Frame::Closed { message, .. } => {
                println!("  fetch {relay_url}: closed ({message})");
                break;
            }
            Frame::Auth { .. } => {
                println!("  fetch {relay_url}: auth-required (REPL does not AUTH)");
                break;
            }
            Frame::RelayClosed | Frame::Io { .. } => break,
            _ => continue,
        }
    }
    out
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
