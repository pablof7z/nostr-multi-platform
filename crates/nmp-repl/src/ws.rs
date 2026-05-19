//! Tungstenite transport helpers. Lifted from
//! `crates/nmp-core/examples/outbox_perf.rs` lines 415–651, kept in lockstep
//! with that reference. The REPL is "outbox_perf, behind a line editor".
//!
//! ## Typed frames (the swallowing-bug fix)
//!
//! The old `next_text` collapsed every non-`Text` message into `None`
//! (benign) or `Some("")` (close/error) — lossy. A relay rate-limiting us
//! with `["AUTH",…]` + `["CLOSED",sub,"auth-required: rate limit exceeded"]`
//! looked identical to "nothing arrived", so every read loop burned its full
//! wall deadline on a silent zero result.
//!
//! [`next_frame`] now parses the JSON envelope **once, here**, and returns a
//! typed [`Frame`]. Callers no longer re-parse and no longer have to
//! distinguish a benign poll-timeout from a socket close by string-matching
//! an empty string. CLOSED / AUTH / NOTICE / a relay-initiated WS Close are
//! each first-class and terminal-aware at every call site.

use std::io::ErrorKind;
use std::net::TcpStream;
use std::time::Duration;

use serde_json::Value;
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

pub type Sock = WebSocket<MaybeTlsStream<TcpStream>>;

/// Per-read poll interval. Keeps reads cooperative so the wall deadline
/// gets enforced promptly. Matches `outbox_perf.rs:48`.
pub const READ_POLL: Duration = Duration::from_millis(250);

/// One decoded relay → client frame. Parsed once in [`next_frame`] so no
/// caller re-parses and none can silently swallow a terminal frame.
#[derive(Debug, Clone)]
pub enum Frame {
    /// `["EVENT", <sub_id>, <event>]`.
    Event { sub_id: String, event: Value },
    /// `["EOSE", <sub_id>]` — normal stored-events end. Terminal for the sub.
    Eose { sub_id: String },
    /// `["CLOSED", <sub_id>, <message>]` — relay closed the sub. Terminal for
    /// the sub; `message` is surfaced verbatim (e.g. `auth-required: rate
    /// limit exceeded`).
    Closed { sub_id: String, message: String },
    /// `["AUTH", <challenge>]` — NIP-42 challenge. The REPL is read-only and
    /// will NOT respond; treat as terminal for any in-flight sub on this
    /// socket and surface it so the user knows why a relay returned nothing.
    Auth { challenge: String },
    /// `["NOTICE", <message>]` — relay notice. Non-terminal (keep reading)
    /// unless followed by a close.
    Notice { message: String },
    /// A well-formed envelope we don't act on (OK, EVENT for an unknown sub,
    /// etc.). Non-terminal — keep reading.
    Other,
    /// The relay sent a WebSocket Close frame: the socket itself is going
    /// away. Terminal for everything on this socket.
    RelayClosed,
    /// Connect/IO failure observed mid-read (socket dropped, reset, etc.).
    /// Terminal for everything on this socket.
    Io { kind: ErrorKind },
    /// Benign read-poll timeout (`WouldBlock`/`TimedOut`). NOT terminal —
    /// the caller loops and re-polls until its own wall deadline.
    Timeout,
}

/// Connect; panic on failure. The REPL prefers `try_connect`; this is a
/// convenience for the cold-start indexer dial where a panic is acceptable.
#[allow(dead_code)]
pub fn connect(url: &str) -> Sock {
    try_connect(url).unwrap_or_else(|| {
        eprintln!("connect failed: {url}");
        std::process::exit(1);
    })
}

/// Try to connect; return `Err(message)` on any failure (DNS, TLS, refused,
/// etc.) so the caller can surface *why* the dial failed rather than a bare
/// "connect refused".
pub fn try_connect_msg(url: &str) -> std::result::Result<Sock, String> {
    let (socket, _response) = match tungstenite::connect(url) {
        Ok(p) => p,
        Err(e) => return Err(connect_err_msg(&e)),
    };
    let _ = match socket.get_ref() {
        MaybeTlsStream::Plain(s) => s.set_read_timeout(Some(READ_POLL)),
        MaybeTlsStream::Rustls(s) => s.get_ref().set_read_timeout(Some(READ_POLL)),
        _ => Ok(()),
    };
    Ok(socket)
}

/// Try to connect; return `None` on any failure. Thin wrapper over
/// [`try_connect_msg`] for call sites that don't surface the reason.
pub fn try_connect(url: &str) -> Option<Sock> {
    try_connect_msg(url).ok()
}

/// Human-readable one-liner for a tungstenite connect error (DNS / TLS /
/// refused / HTTP upgrade rejected).
fn connect_err_msg(e: &tungstenite::Error) -> String {
    match e {
        tungstenite::Error::Io(io) => format!("{}", io.kind()),
        tungstenite::Error::Tls(t) => format!("TLS error: {t}"),
        tungstenite::Error::Http(resp) => {
            format!("HTTP {}", resp.status())
        }
        // tungstenite folds connect failures (DNS/timeout/refused) into
        // `Url(UnableToConnect(host:port))` — surface its real message, not
        // a misleading "bad url".
        tungstenite::Error::Url(u) => u.to_string(),
        other => {
            let s = other.to_string();
            if s.len() > 80 {
                format!("{}…", &s[..79])
            } else {
                s
            }
        }
    }
}

/// Read one frame from the socket, parsing the JSON envelope here so no
/// caller re-parses or swallows a terminal frame. See [`Frame`].
pub fn next_frame(socket: &mut Sock) -> Frame {
    match socket.read() {
        Ok(Message::Text(s)) => parse_envelope(&s),
        Ok(Message::Close(_)) => Frame::RelayClosed,
        Ok(_) => Frame::Other,
        Err(tungstenite::Error::Io(e))
            if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut =>
        {
            Frame::Timeout
        }
        Err(tungstenite::Error::Io(e)) => Frame::Io { kind: e.kind() },
        Err(tungstenite::Error::ConnectionClosed)
        | Err(tungstenite::Error::AlreadyClosed) => Frame::RelayClosed,
        Err(_) => Frame::Io {
            kind: ErrorKind::Other,
        },
    }
}

/// Parse a relay → client JSON envelope into a [`Frame`]. Unknown / malformed
/// envelopes become [`Frame::Other`] (non-terminal, keep reading).
fn parse_envelope(text: &str) -> Frame {
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return Frame::Other,
    };
    match v.get(0).and_then(Value::as_str) {
        Some("EVENT") => match (v.get(1).and_then(Value::as_str), v.get(2)) {
            (Some(sub), Some(event)) => Frame::Event {
                sub_id: sub.to_string(),
                event: event.clone(),
            },
            _ => Frame::Other,
        },
        Some("EOSE") => match v.get(1).and_then(Value::as_str) {
            Some(sub) => Frame::Eose {
                sub_id: sub.to_string(),
            },
            None => Frame::Other,
        },
        Some("CLOSED") => Frame::Closed {
            sub_id: v
                .get(1)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            message: v
                .get(2)
                .and_then(Value::as_str)
                .unwrap_or("closed")
                .to_string(),
        },
        Some("AUTH") => Frame::Auth {
            challenge: v
                .get(1)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        },
        Some("NOTICE") => Frame::Notice {
            message: v
                .get(1)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        },
        _ => Frame::Other,
    }
}

/// Normalise a relay URL. Strips trailing slashes (except the "://" one),
/// trims whitespace, rejects non-ws schemes. Lifted from
/// `outbox_perf.rs:415`.
pub fn normalize_url(s: &str) -> String {
    let trimmed = s.trim();
    if !(trimmed.starts_with("wss://") || trimmed.starts_with("ws://")) {
        return String::new();
    }
    let mut s = trimmed.to_string();
    while s.ends_with('/') && s.matches('/').count() > 2 {
        s.pop();
    }
    if s.ends_with('/') {
        s.pop();
    }
    s
}

/// Compact human summary of a REQ filter JSON for the per-REQ row label,
/// e.g. `kind:1 (83 authors)`, `kind:10002 (50 authors)`, `kind:3 (1
/// author)`. Shared by all three relay-interaction sites so the rendering
/// is identical everywhere.
pub fn summarize_filter(filter_json: &str) -> String {
    let v: Value = match serde_json::from_str(filter_json) {
        Ok(v) => v,
        Err(_) => return "filter:?".to_string(),
    };
    let kinds: Vec<String> = v
        .get("kinds")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_u64).map(|k| k.to_string()).collect())
        .unwrap_or_default();
    let kind_part = match kinds.len() {
        0 => "kind:any".to_string(),
        1 => format!("kind:{}", kinds[0]),
        _ => format!("kinds:[{}]", kinds.join(",")),
    };
    let authors = v
        .get("authors")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    if authors == 0 {
        kind_part
    } else if authors == 1 {
        format!("{kind_part} (1 author)")
    } else {
        format!("{kind_part} ({authors} authors)")
    }
}

/// Truncate `s` to at most `n` chars, appending an ellipsis if truncated.
/// Used by the renderer; lifted from `outbox_perf.rs:653`.
pub fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let cut: String = s.chars().take(n.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}
