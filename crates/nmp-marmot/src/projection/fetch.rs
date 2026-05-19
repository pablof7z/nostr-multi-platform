//! Minimal synchronous WebSocket fetch helper for `poll_inbox`.
//!
//! Issues a single REQ against one relay, collects EVENTs until EOSE (or
//! wall deadline), closes. No retry, no outbox routing. Mirrors the logic in
//! `nmp-repl/src/publish.rs:fetch_events`.

use std::io::ErrorKind;
use std::net::TcpStream;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

type Sock = WebSocket<MaybeTlsStream<TcpStream>>;
const READ_POLL: Duration = Duration::from_millis(250);

/// Fetch events matching `filter_json` from `relay_url`, collecting until
/// EOSE or `wall`. Returns the raw event `Value`s (the event object, not the
/// full `["EVENT", sub, …]` envelope).
pub fn fetch_events(relay_url: &str, filter_json: &Value, wall: Duration) -> Vec<Value> {
    let sub = format!("marmot-poll-{}", now_secs());
    let mut out = Vec::new();

    let mut sock = match connect(relay_url) {
        Ok(s) => s,
        Err(_) => return out,
    };

    let req = json!(["REQ", sub, filter_json]).to_string();
    if sock.send(Message::Text(req)).is_err() {
        return out;
    }

    let deadline = Instant::now() + wall;
    while Instant::now() < deadline {
        match sock.read() {
            Ok(Message::Text(s)) => match parse(&s) {
                Parsed::Event { sub_id, event } if sub_id == sub => out.push(event),
                Parsed::Eose { sub_id } if sub_id == sub => break,
                Parsed::Closed { .. } | Parsed::Auth => break,
                _ => {}
            },
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {}
            Err(_) => break,
        }
    }
    out
}

fn connect(url: &str) -> Result<Sock, ()> {
    let (socket, _) = tungstenite::connect(url).map_err(|_| ())?;
    let _ = match socket.get_ref() {
        MaybeTlsStream::Plain(s) => s.set_read_timeout(Some(READ_POLL)),
        MaybeTlsStream::Rustls(s) => s.get_ref().set_read_timeout(Some(READ_POLL)),
        _ => Ok(()),
    };
    Ok(socket)
}

enum Parsed {
    Event { sub_id: String, event: Value },
    Eose { sub_id: String },
    Closed { _sub_id: String },
    Auth,
    Other,
}

fn parse(text: &str) -> Parsed {
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return Parsed::Other,
    };
    match v.get(0).and_then(Value::as_str) {
        Some("EVENT") => match (v.get(1).and_then(Value::as_str), v.get(2)) {
            (Some(sub), Some(ev)) => Parsed::Event {
                sub_id: sub.to_string(),
                event: ev.clone(),
            },
            _ => Parsed::Other,
        },
        Some("EOSE") => Parsed::Eose {
            sub_id: v.get(1).and_then(Value::as_str).unwrap_or("").to_string(),
        },
        Some("CLOSED") => Parsed::Closed {
            _sub_id: v.get(1).and_then(Value::as_str).unwrap_or("").to_string(),
        },
        Some("AUTH") => Parsed::Auth,
        _ => Parsed::Other,
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
