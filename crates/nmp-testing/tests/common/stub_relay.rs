//! TCP stub relay for A5 (mid-claim unreachable).
//!
//! Binds `127.0.0.1:0` (ephemeral port), performs the WebSocket handshake
//! on the first connection, then drops the connection after a configurable
//! delay without sending any frames.  This simulates a relay that accepts
//! the TCP + WebSocket upgrade but then becomes unreachable before it can
//! respond to a `REQ`.
//!
//! # Usage
//!
//! ```ignore
//! let stub = StubRelay::spawn(Duration::from_millis(50));
//! let ws_url = stub.ws_url(); // e.g. "ws://127.0.0.1:54321"
//! // ... register stub.ws_url() as an app relay and claim an event ...
//! // stub drops when it goes out of scope; the background thread exits.
//! ```

use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// A stub WebSocket relay that drops connections after `drop_after`.
pub(crate) struct StubRelay {
    /// The bound local address (127.0.0.1:<ephemeral_port>).
    local_addr: std::net::SocketAddr,
    /// Signal to stop the acceptor thread.
    stop: Arc<AtomicBool>,
}

impl StubRelay {
    /// Spawn the stub relay.  Bind to an ephemeral port and start an acceptor
    /// thread that immediately drops connections after `drop_after`.
    pub(crate) fn spawn(drop_after: Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("stub relay: bind failed");
        let local_addr = listener
            .local_addr()
            .expect("stub relay: local_addr failed");
        listener
            .set_nonblocking(false)
            .expect("stub relay: set_nonblocking failed");

        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);

        thread::spawn(move || {
            listener
                .set_nonblocking(true)
                .expect("stub relay: set_nonblocking(true) failed");
            loop {
                if stop_clone.load(Ordering::Relaxed) {
                    return;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        let delay = drop_after;
                        thread::spawn(move || handle_connection(stream, delay));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => return,
                }
            }
        });

        Self { local_addr, stop }
    }

    /// The `ws://` URL tests should pass as an app relay URL.
    pub(crate) fn ws_url(&self) -> String {
        format!("ws://127.0.0.1:{}", self.local_addr.port())
    }
}

impl Drop for StubRelay {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Accept the WebSocket upgrade, then sleep `delay` and close the connection.
fn handle_connection(stream: TcpStream, delay: Duration) {
    stream
        .set_nonblocking(false)
        .expect("stub relay handle: set_nonblocking failed");
    // Perform the WebSocket handshake so the kernel's relay worker believes
    // it is connected before the drop.  `tungstenite::accept` completes
    // the HTTP-upgrade exchange over the TCP stream.
    match tungstenite::accept(stream) {
        Ok(_ws) => {
            // WebSocket handshake complete.  Sleep then drop `_ws`, which
            // closes the underlying TCP connection — the kernel will see a
            // broken-pipe / connection-reset on its next read or write.
            thread::sleep(delay);
            // `_ws` is dropped here, closing the connection.
        }
        Err(_) => {
            // Handshake failed — connection is already dropped by tungstenite.
        }
    }
}
