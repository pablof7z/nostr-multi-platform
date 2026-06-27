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

/// Resolve the handshake User-Agent: the configured override, or the built-in
/// `nmp/<ver>` fallback when none was supplied.
pub(super) fn resolve_user_agent(user_agent: Option<&str>) -> std::borrow::Cow<'static, str> {
    match user_agent {
        Some(ua) => std::borrow::Cow::Owned(ua.to_string()),
        None => std::borrow::Cow::Borrowed(concat!("nmp/", env!("CARGO_PKG_VERSION"))),
    }
}

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
pub(super) fn open_relay_socket(
    relay_url: &str,
    user_agent: Option<&str>,
) -> Result<RelaySocket, String> {
    install_rustls_provider();

    let mut request = relay_url
        .into_client_request()
        .map_err(|error| error.to_string())?;
    // Identify the client to the relay. Some relays (e.g. nostr.wine) reject
    // the bare handshake with HTTP 403 unless the client sends a `User-Agent`
    // — a NIP-50 search that resolves to such a relay would otherwise fail to
    // connect at all. Sending a UA is strictly additive: no relay rejects a
    // request *for* carrying one. The UA is either the configured override
    // (Flow A: from ClientIdentity) or the built-in `nmp/<ver>` fallback.
    let ua = resolve_user_agent(user_agent);
    request.headers_mut().insert(
        "User-Agent",
        tungstenite::http::HeaderValue::from_str(&ua).unwrap_or_else(|_| {
            tungstenite::http::HeaderValue::from_static(concat!("nmp/", env!("CARGO_PKG_VERSION")))
        }),
    );
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
    //
    // KNOWN BOUNDED RESIDUAL: these are per-syscall inactivity timeouts, not a
    // total-handshake deadline. A maliciously slow-trickling peer that sends a
    // byte just under `TCP_CONNECT_TIMEOUT` of inactivity, repeatedly, never
    // completes the upgrade yet never trips the timeout — so the worker (and its
    // detached reaper join) can outlive a cancel for as long as the peer keeps
    // trickling. This is rare, requires an adversarial relay, leaks at most ONE
    // such worker thread per affected cancel, and NEVER blocks the actor/caller
    // (cancel is detached). Closing it fully needs a total-deadline /
    // interruptible-socket rewrite (out of D4 scope); the per-syscall bound here
    // plus the bounded DNS resolve (`resolve_with_deadline`) cover the common
    // stuck-connect cases.
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

/// Resolve `host:port` (DNS) within `timeout`, then `TcpStream::connect_timeout`
/// to the first address that connects within `timeout`. The connect timeout
/// applies per resolved address, matching `TcpStream::connect_timeout`'s
/// contract; we try addresses in turn so a host with both a stuck and a live
/// address still connects.
///
/// DNS resolution (`to_socket_addrs` → blocking `getaddrinfo`) is itself
/// unbounded and can wedge the worker for the OS resolver's default — during
/// which the worker cannot observe `WorkerCmd::Shutdown`, stalling
/// `shutdown()`/`cancel()` teardown. We bound the COMMON stuck-DNS case by
/// resolving on a helper thread and waiting only `timeout` for it: on timeout
/// we abandon that one helper and fail the connect, so the worker returns to
/// its control poll and exits on Shutdown. The abandoned resolver thread is
/// parked in `getaddrinfo` (not spinning) and self-completes when the OS
/// resolver eventually returns, so it does not leak unboundedly — at most one
/// such thread per stuck dial, and it never blocks the caller.
///
/// Note: a fully-interruptible `getaddrinfo` is a separate, large effort (out
/// of scope here); this deadline bound is the tractable mitigation.
fn connect_with_timeout(host: &str, port: u16, timeout: Duration) -> std::io::Result<TcpStream> {
    let addrs = resolve_with_deadline(host, port, timeout)?;
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

/// Resolve `host:port` to socket addresses, bounded by `deadline`. Runs the
/// blocking `to_socket_addrs` on a detached helper thread and waits at most
/// `deadline` for the result; on timeout returns a `TimedOut` error and the
/// helper is abandoned (it self-completes when the OS resolver returns — parked
/// in `getaddrinfo`, not spinning, so no unbounded leak).
fn resolve_with_deadline(
    host: &str,
    port: u16,
    deadline: Duration,
) -> std::io::Result<std::vec::IntoIter<std::net::SocketAddr>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let host_owned = host.to_string();
    // Detached helper owns the blocking getaddrinfo. On timeout we drop `rx`;
    // the helper's `send` then fails silently and it exits when the OS resolver
    // returns — no spin, no unbounded leak, never blocks the caller.
    let _ = std::thread::Builder::new()
        .name("nmp-relay-dns".to_string())
        .spawn(move || {
            let result = (host_owned.as_str(), port)
                .to_socket_addrs()
                .map(|addrs| addrs.collect::<Vec<_>>());
            let _ = tx.send(result);
        });
    match rx.recv_timeout(deadline) {
        Ok(Ok(addrs)) => Ok(addrs.into_iter()),
        Ok(Err(error)) => Err(error),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("dns resolution for {host}:{port} exceeded {deadline:?}"),
        )),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(std::io::Error::other(
            "dns resolver thread terminated unexpectedly",
        )),
    }
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
        assert!(
            result.is_err(),
            "black-holed connect must fail, not succeed"
        );
        assert!(
            elapsed < Duration::from_secs(10),
            "connect took {elapsed:?}; the timeout bound is not in effect \
             (regressed toward the OS default ~75s)"
        );
    }

    /// `resolve_with_deadline` must return inside its deadline when DNS hangs.
    /// We can't force a real `getaddrinfo` to hang deterministically, so we
    /// drive the deadline path with a near-zero deadline against a resolvable
    /// host: the helper's `to_socket_addrs` cannot complete within the deadline,
    /// so the function must return a `TimedOut` error promptly rather than
    /// blocking on the OS resolver. Pins the DNS-bounding fix — a stuck resolver
    /// can no longer wedge the worker past the deadline (the common stuck-DNS
    /// case from the cancel-teardown rework).
    #[test]
    fn resolve_with_deadline_is_bounded_on_slow_dns() {
        let started = Instant::now();
        // Sub-millisecond deadline: the spawned resolver cannot finish
        // getaddrinfo (even a cached localhost lookup needs > 1µs of thread
        // spawn + send), so the recv_timeout fires first.
        let result = resolve_with_deadline("example.com", 443, Duration::from_nanos(1));
        let elapsed = started.elapsed();
        assert!(
            result.is_err(),
            "a sub-deadline resolve must time out, not block on the OS resolver"
        );
        assert_eq!(
            result.unwrap_err().kind(),
            std::io::ErrorKind::TimedOut,
            "deadline overrun must surface as TimedOut so the worker fails the connect fast"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "resolve returned in {elapsed:?}; the deadline bound is not in effect"
        );
    }

    /// A resolvable host with a generous deadline still resolves successfully —
    /// the deadline wrapper must not break the happy path.
    #[test]
    fn resolve_with_deadline_resolves_localhost() {
        let addrs = resolve_with_deadline("127.0.0.1", 443, Duration::from_secs(5))
            .expect("loopback literal must resolve within the deadline");
        assert!(
            addrs.count() >= 1,
            "127.0.0.1 must resolve to at least one socket address"
        );
    }

    #[cfg(test)]
    mod ua_tests {
        use super::*;

        #[test]
        fn ua_fallback_when_none() {
            let result = resolve_user_agent(None);
            assert_eq!(result, concat!("nmp/", env!("CARGO_PKG_VERSION")));
        }

        #[test]
        fn ua_uses_configured_value() {
            let result = resolve_user_agent(Some("Chirp/1.2.0 (nmp/0.8.0)"));
            assert_eq!(result, "Chirp/1.2.0 (nmp/0.8.0)");
        }
    }
}
