//! Bounded relay socket dialing for the relay worker.
//!
//! Split out of `relay_worker::mod` (file-size ownership): establishing the
//! TCP stream with a *bounded* `TcpStream::connect_timeout` and upgrading it to
//! a WebSocket is one cohesive unit — the only place this crate dials a relay.
//!
//! The bound is load-bearing for teardown: the blocking `tungstenite::connect`
//! helper dials with an unbounded `TcpStream::connect`, so a relay that accepts
//! SYNs but never finishes the handshake (or a black-holed route) wedges the
//! worker for the full OS connect timeout (~75 s). A pending
//! `WorkerCmd::Shutdown` then sits in the control channel until that returns,
//! making `shutdown()`/`cancel()` hostage to the OS default. Bounding the
//! connect here caps the worst-case teardown latency.

use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Once;
use std::time::Duration;

use tungstenite::client::{uri_mode, IntoClientRequest};
use tungstenite::error::{Error as WsError, UrlError};
use tungstenite::stream::Mode;
use tungstenite::{client_tls_with_config, HandshakeError};

use super::RelaySocket;

/// Upper bound on the OS-level TCP connect for a single relay dial. 10 s
/// comfortably covers a reachable relay's TCP+TLS handshake on a slow network.
const TCP_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Dial `relay_url`, returning a ready WebSocket.
///
/// Unlike `tungstenite::connect`, we establish the TCP stream with
/// `TcpStream::connect_timeout` so a stuck connect can never wedge the worker —
/// and therefore can never delay `shutdown()`/`cancel()` past
/// [`TCP_CONNECT_TIMEOUT`]. The WebSocket upgrade (TLS + HTTP) then runs on the
/// already-connected stream via `client_tls_with_config`, the exact helper
/// `tungstenite::connect` calls internally once it has a stream — so the
/// upgrade behaviour, the rustls path, and the HTTP-denial error strings
/// (`401`/`403`/`Forbidden`, classified by `is_permanent_error`) are unchanged.
pub(super) fn open_relay_socket(relay_url: &str) -> Result<RelaySocket, String> {
    install_rustls_provider();

    let request = relay_url
        .into_client_request()
        .map_err(|error| error.to_string())?;
    let uri = request.uri();
    let mode = uri_mode(uri).map_err(|error| error.to_string())?;

    let host = uri
        .host()
        .ok_or_else(|| WsError::Url(UrlError::NoHostName).to_string())?;
    // Strip IPv6 brackets, mirroring tungstenite's own connect path.
    let host = if host.starts_with('[') && host.ends_with(']') {
        &host[1..host.len() - 1]
    } else {
        host
    };
    let port = uri.port_u16().unwrap_or(match mode {
        Mode::Plain => 80,
        Mode::Tls => 443,
    });

    let stream = connect_with_timeout(host, port, TCP_CONNECT_TIMEOUT)
        .map_err(|error| format!("tcp connect {host}:{port}: {error}"))?;
    // Match tungstenite's connect helper: disable Nagle so handshake frames
    // are not delayed.
    stream
        .set_nodelay(true)
        .map_err(|error| format!("set_nodelay: {error}"))?;

    // Bound the TLS + HTTP-upgrade handshake. A relay that completes the TCP
    // handshake but then stalls the TLS/HTTP upgrade (slow or black-holed after
    // SYN-ACK) would otherwise wedge `client_tls_with_config`'s blocking reads
    // indefinitely — outside `TCP_CONNECT_TIMEOUT`, which only covers the TCP
    // connect. While wedged the worker cannot observe `WorkerCmd::Shutdown`, so
    // `shutdown()`/`cancel()` teardown (and the cancel reaper) would hang on it.
    // Setting per-syscall read/write timeouts caps each blocking handshake
    // read/write at `TCP_CONNECT_TIMEOUT`, so the upgrade fails fast and the
    // worker returns to its control-channel poll. This timeout does NOT leak
    // into steady state: `RelayPoller::new` unconditionally puts the socket into
    // non-blocking mode (`set_nonblocking(true)`) before the readiness loop, so
    // the steady-state path stays readiness-driven (no polling).
    stream
        .set_read_timeout(Some(TCP_CONNECT_TIMEOUT))
        .map_err(|error| format!("set handshake read timeout: {error}"))?;
    stream
        .set_write_timeout(Some(TCP_CONNECT_TIMEOUT))
        .map_err(|error| format!("set handshake write timeout: {error}"))?;

    // Upgrade the already-connected stream to a WebSocket (TLS via the rustls
    // feature). On HTTP rejection the error carries the status code, so
    // `is_permanent_error` keeps classifying 401/403 as permanent.
    let (socket, _response) =
        client_tls_with_config(request, stream, None, None).map_err(|error| match error {
            HandshakeError::Failure(f) => f.to_string(),
            // Blocking stream → the handshake cannot return Interrupted.
            HandshakeError::Interrupted(_) => {
                "handshake interrupted on blocking stream".to_string()
            }
        })?;
    Ok(socket)
}

/// Resolve `host:port` and `TcpStream::connect_timeout` to the first address
/// that connects within `timeout`. The timeout applies per resolved address,
/// matching `TcpStream::connect_timeout`'s contract; we try addresses in turn
/// so a host with both a stuck and a live address still connects.
fn connect_with_timeout(host: &str, port: u16, timeout: Duration) -> std::io::Result<TcpStream> {
    let addrs = (host, port).to_socket_addrs()?;
    let mut last_err: Option<std::io::Error> = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(stream) => return Ok(stream),
            Err(error) => last_err = Some(error),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            "no addresses resolved for host",
        )
    }))
}

fn install_rustls_provider() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// `connect_with_timeout` against a black-holed address must return inside
    /// its bound, never the OS default (~75 s). RFC 5737 TEST-NET-1
    /// (`192.0.2.1`) is reserved and non-routable: SYNs are dropped, so an
    /// unbounded connect would hang for the full OS timeout. Pins the defect
    /// fix — a stuck dial can no longer wedge the worker and stall
    /// `shutdown()`/`cancel()`.
    #[test]
    fn connect_with_timeout_is_bounded_not_os_default() {
        let started = Instant::now();
        let result = connect_with_timeout("192.0.2.1", 9, Duration::from_secs(2));
        let elapsed = started.elapsed();
        assert!(result.is_err(), "black-holed connect must fail, not succeed");
        assert!(
            elapsed < Duration::from_secs(10),
            "connect took {elapsed:?}; the timeout bound is not in effect \
             (regressed toward the OS default ~75s)"
        );
    }
}
