//! Minimal synchronous WebSocket helper for `publish_key_package`.
//!
//! `send_event` sends an EVENT, waits for OK/NOTICE or a wall deadline, then
//! closes the socket. It does not open read subscriptions; inbound MLS data is
//! delivered through kernel-managed interests and the raw-event tap.
//!
//! ## D6 — error propagation
//!
//! Both helpers return `Result<_, String>` so a connection failure is
//! distinguishable from "no events" / "relay rejected the event". The
//! string is surfaced verbatim into the `publish_key_package` op envelope — no
//! panic crosses the FFI boundary.

use std::io::ErrorKind;
use std::net::TcpStream;
use std::time::{Duration, Instant};

use serde_json::Value;
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

type Sock = WebSocket<MaybeTlsStream<TcpStream>>;
const READ_TIMEOUT: Duration = Duration::from_millis(250);

/// Publish a signed event to `relay_url`. Waits up to `wall` for an OK/NOTICE.
/// Returns `Ok(true)` on relay acceptance, `Ok(false)` on rejection / NOTICE /
/// timeout (the relay was reached but did not confirm), `Err(String)` on
/// connection / send failure (D6 — the caller can tell the two apart).
pub fn send_event(relay_url: &str, event_json: &str, wall: Duration) -> Result<bool, String> {
    let mut sock = connect(relay_url)?;
    let msg = format!("[\"EVENT\",{}]", event_json);
    if let Err(e) = sock.send(Message::Text(msg)) {
        let _ = sock.close(None);
        return Err(format!("EVENT send to {relay_url} failed: {e}"));
    }

    let mut accepted = false;
    let deadline = Instant::now() + wall;
    while Instant::now() < deadline {
        match sock.read() {
            Ok(Message::Text(s)) => {
                let v: Value = match serde_json::from_str(&s) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                match v.get(0).and_then(Value::as_str) {
                    Some("OK") => {
                        accepted = v.get(2).and_then(Value::as_bool).unwrap_or(true);
                        break;
                    }
                    Some("NOTICE") => break,
                    _ => {}
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {}
            Err(_) => break,
        }
    }
    close(&mut sock);
    Ok(accepted)
}

fn connect(url: &str) -> Result<Sock, String> {
    let (socket, _) =
        tungstenite::connect(url).map_err(|e| format!("connect {url} failed: {e}"))?;
    let _ = match socket.get_ref() {
        MaybeTlsStream::Plain(s) => s.set_read_timeout(Some(READ_TIMEOUT)),
        MaybeTlsStream::Rustls(s) => s.get_ref().set_read_timeout(Some(READ_TIMEOUT)),
        _ => Ok(()),
    };
    Ok(socket)
}

/// Send a Close frame and flush it. Best-effort: a relay that already closed
/// the TCP stream makes this a no-op. Keeps the relay from leaking the REQ
/// subscription until its own idle timeout fires.
fn close(sock: &mut Sock) {
    if sock.close(None).is_ok() {
        // `close` only queues the frame; `flush` (or a final `read`) writes
        // it. A read also drains the relay's Close acknowledgement.
        let _ = sock.flush();
    }
}
