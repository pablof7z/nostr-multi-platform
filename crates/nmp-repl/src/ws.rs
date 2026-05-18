//! Tungstenite transport helpers. Lifted from
//! `crates/nmp-core/examples/outbox_perf.rs` lines 415–651, kept in lockstep
//! with that reference. The REPL is "outbox_perf, behind a line editor".

use std::io::ErrorKind;
use std::net::TcpStream;
use std::time::Duration;

use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

pub type Sock = WebSocket<MaybeTlsStream<TcpStream>>;

/// Per-read poll interval. Keeps reads cooperative so the wall deadline
/// gets enforced promptly. Matches `outbox_perf.rs:48`.
pub const READ_POLL: Duration = Duration::from_millis(250);

/// Connect; panic on failure. The REPL prefers `try_connect`; this is a
/// convenience for the cold-start indexer dial where a panic is acceptable.
pub fn connect(url: &str) -> Sock {
    try_connect(url).unwrap_or_else(|| {
        eprintln!("connect failed: {url}");
        std::process::exit(1);
    })
}

/// Try to connect; return `None` on any failure (DNS, TLS, refused, etc.).
pub fn try_connect(url: &str) -> Option<Sock> {
    let (socket, _response) = match tungstenite::connect(url) {
        Ok(p) => p,
        Err(_) => return None,
    };
    let _ = match socket.get_ref() {
        MaybeTlsStream::Plain(s) => s.set_read_timeout(Some(READ_POLL)),
        MaybeTlsStream::Rustls(s) => s.get_ref().set_read_timeout(Some(READ_POLL)),
        _ => Ok(()),
    };
    Some(socket)
}

/// Read one text frame from the socket. Returns `Some(text)` on success,
/// `None` on a benign timeout (WouldBlock / TimedOut). Returns
/// `Some(String::new())` to signal "close / error — drain loop should bail
/// or treat as inert" — matches `outbox_perf.rs:651`.
pub fn next_text(socket: &mut Sock) -> Option<String> {
    match socket.read() {
        Ok(Message::Text(s)) => Some(s),
        Ok(Message::Close(_)) => Some(String::new()),
        Ok(_) => None,
        Err(tungstenite::Error::Io(e))
            if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut =>
        {
            None
        }
        Err(_) => Some(String::new()),
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
