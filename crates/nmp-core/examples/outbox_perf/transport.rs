//! Low-level WebSocket transport helpers shared by all phases.

use std::io::ErrorKind;
use std::net::TcpStream;
use std::time::Duration;

use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

/// Convenience alias used throughout the example.
pub type Sock = WebSocket<MaybeTlsStream<TcpStream>>;

/// Timeout applied to every socket read so phases can poll without blocking.
pub const READ_POLL: Duration = Duration::from_millis(250);

/// Connect to `url`, applying `READ_POLL` timeout; exit the process on failure.
pub fn connect(url: &str) -> Sock {
    try_connect(url).unwrap_or_else(|| {
        eprintln!("connect failed: {url}");
        std::process::exit(1);
    })
}

/// Try to connect to `url`; returns `None` on any error.
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

/// Read the next text frame; returns `None` for non-text frames and
/// transient I/O timeouts (so callers can poll without blocking).
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

/// Truncate a string to at most `n` bytes (appending `…` when truncated).
pub fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n.saturating_sub(1)])
    }
}
