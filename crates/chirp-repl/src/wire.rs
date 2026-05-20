use std::net::TcpStream;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

type Sock = WebSocket<MaybeTlsStream<TcpStream>>;

pub fn fetch(relays: &[String], filter: Value, wall: Duration) -> Vec<Value> {
    let sub = format!("chirp-repl-{}", now_secs());
    let frame = json!(["REQ", sub, filter]).to_string();
    let mut out = Vec::new();
    for relay in relays {
        println!("  -> {relay}");
        match connect(relay) {
            Ok(mut sock) => {
                if let Err(e) = sock.send(Message::Text(frame.clone())) {
                    println!("     send failed: {e}");
                    continue;
                }
                out.extend(read_events(&mut sock, &sub, wall));
            }
            Err(e) => println!("     connect failed: {e}"),
        }
    }
    out
}

pub fn publish(event: &nostr::Event, relays: &[String], wall: Duration) -> (usize, usize) {
    let id = event.id.to_hex();
    let frame = json!(["EVENT", event]).to_string();
    let mut ok = 0;
    let mut fail = 0;
    for relay in relays {
        match connect(relay) {
            Ok(mut sock) => {
                if let Err(e) = sock.send(Message::Text(frame.clone())) {
                    println!("  {relay}: send failed: {e}");
                    fail += 1;
                    continue;
                }
                match read_ok(&mut sock, &id, wall) {
                    OkReply::Accepted | OkReply::NoReply => {
                        println!("  {relay}: sent");
                        ok += 1;
                    }
                    OkReply::Rejected(message) => {
                        println!("  {relay}: rejected: {message}");
                        fail += 1;
                    }
                }
            }
            Err(e) => {
                println!("  {relay}: connect failed: {e}");
                fail += 1;
            }
        }
    }
    (ok, fail)
}

fn connect(url: &str) -> Result<Sock, String> {
    let (sock, _) = tungstenite::connect(url).map_err(|e| e.to_string())?;
    let timeout = Some(Duration::from_millis(250));
    match sock.get_ref() {
        MaybeTlsStream::Plain(s) => s.set_read_timeout(timeout).map_err(|e| e.to_string())?,
        MaybeTlsStream::Rustls(s) => s
            .get_ref()
            .set_read_timeout(timeout)
            .map_err(|e| e.to_string())?,
        _ => {}
    }
    Ok(sock)
}

fn read_events(sock: &mut Sock, sub: &str, wall: Duration) -> Vec<Value> {
    let deadline = Instant::now() + wall;
    let mut events = Vec::new();
    while Instant::now() < deadline {
        match sock.read() {
            Ok(Message::Text(text)) => match parse_frame(&text) {
                Frame::Event { sub_id, event } if sub_id == sub => events.push(event),
                Frame::Eose { sub_id } if sub_id == sub => break,
                Frame::Closed(message) => {
                    println!("     CLOSED {message}");
                    break;
                }
                Frame::Auth(challenge) => {
                    println!("     AUTH required {}", short(&challenge));
                    break;
                }
                Frame::Notice(message) => println!("     NOTICE {message}"),
                _ => {}
            },
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => break,
        }
    }
    events
}

enum OkReply {
    Accepted,
    Rejected(String),
    NoReply,
}

fn read_ok(sock: &mut Sock, event_id: &str, wall: Duration) -> OkReply {
    let deadline = Instant::now() + wall;
    while Instant::now() < deadline {
        match sock.read() {
            Ok(Message::Text(text)) => {
                let Ok(v) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                if v.get(0).and_then(Value::as_str) == Some("OK")
                    && v.get(1).and_then(Value::as_str) == Some(event_id)
                {
                    return if v.get(2).and_then(Value::as_bool).unwrap_or(false) {
                        OkReply::Accepted
                    } else {
                        OkReply::Rejected(v.get(3).and_then(Value::as_str).unwrap_or("").into())
                    };
                }
            }
            Ok(Message::Close(_)) => return OkReply::NoReply,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => return OkReply::NoReply,
        }
    }
    OkReply::NoReply
}

enum Frame {
    Event { sub_id: String, event: Value },
    Eose { sub_id: String },
    Closed(String),
    Auth(String),
    Notice(String),
    Other,
}

fn parse_frame(text: &str) -> Frame {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Frame::Other;
    };
    match v.get(0).and_then(Value::as_str) {
        Some("EVENT") => match (v.get(1).and_then(Value::as_str), v.get(2)) {
            (Some(sub_id), Some(event)) => Frame::Event {
                sub_id: sub_id.into(),
                event: event.clone(),
            },
            _ => Frame::Other,
        },
        Some("EOSE") => v
            .get(1)
            .and_then(Value::as_str)
            .map(|s| Frame::Eose { sub_id: s.into() })
            .unwrap_or(Frame::Other),
        Some("CLOSED") => Frame::Closed(v.get(2).and_then(Value::as_str).unwrap_or("").into()),
        Some("AUTH") => Frame::Auth(v.get(1).and_then(Value::as_str).unwrap_or("").into()),
        Some("NOTICE") => Frame::Notice(v.get(1).and_then(Value::as_str).unwrap_or("").into()),
        _ => Frame::Other,
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn short(input: &str) -> String {
    if input.len() <= 12 {
        input.into()
    } else {
        format!("{}..", &input[..12])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_event_frame() {
        let raw = json!(["EVENT", "sub1", {"id":"abc", "kind":1}]).to_string();
        match parse_frame(&raw) {
            Frame::Event { sub_id, event } => {
                assert_eq!(sub_id, "sub1");
                assert_eq!(event.get("id").and_then(Value::as_str), Some("abc"));
            }
            _ => panic!("expected event frame"),
        }
    }

    #[test]
    fn parses_terminal_and_diagnostic_frames() {
        match parse_frame(&json!(["EOSE", "sub1"]).to_string()) {
            Frame::Eose { sub_id } => assert_eq!(sub_id, "sub1"),
            _ => panic!("expected eose"),
        }
        match parse_frame(&json!(["CLOSED", "sub1", "rate limit"]).to_string()) {
            Frame::Closed(message) => assert_eq!(message, "rate limit"),
            _ => panic!("expected closed"),
        }
        match parse_frame(&json!(["AUTH", "challenge"]).to_string()) {
            Frame::Auth(challenge) => assert_eq!(challenge, "challenge"),
            _ => panic!("expected auth"),
        }
        match parse_frame(&json!(["NOTICE", "heads up"]).to_string()) {
            Frame::Notice(message) => assert_eq!(message, "heads up"),
            _ => panic!("expected notice"),
        }
    }

    #[test]
    fn malformed_frames_are_other() {
        assert!(matches!(parse_frame("not-json"), Frame::Other));
        assert!(matches!(
            parse_frame(&json!(["EVENT"]).to_string()),
            Frame::Other
        ));
    }
}
