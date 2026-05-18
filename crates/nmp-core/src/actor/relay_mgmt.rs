//! Relay lifecycle helpers — spawning, closing, routing outbound messages.
//!
//! # T105 — URL-keyed transport pool
//!
//! `relay_controls` is keyed by **resolved relay URL**, not by `RelayRole`.
//! `send_outbound` dispatches each `OutboundMessage` by its `relay_url`, and
//! a worker is spawned **on demand** the first time a new URL appears (cold
//! discovery seed at startup, then per resolved write/read relay as the
//! kernel resolves NIP-65 mailboxes). `connected_relays` is still per-`RelayRole`
//! to drive the diagnostic surface (one row per lane) until M11 makes
//! per-URL health a first-class part of the FFI projection.

use crate::kernel::Kernel;
use crate::relay::{OutboundMessage, RelayRole, BOOTSTRAP_DISCOVERY_RELAYS};
use crate::relay_worker::{spawn_relay_worker, RelayCommand, RelayEvent};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Sender;

use super::RelayControl;

/// True when at least one URL on **every** lane has reported `Connected`.
/// Used as the startup-send gate so the first burst of REQs has somewhere to
/// land. Per-lane (`RelayRole`) granularity matches the diagnostic surface;
/// M11 will sharpen this to per-URL once the FFI projection lands.
pub(super) fn all_relays_connected(connected_relays: &HashSet<RelayRole>) -> bool {
    RelayRole::all()
        .into_iter()
        .all(|role| connected_relays.contains(&role))
}

/// Lane-bootstrap seeds: spawn one worker per `BOOTSTRAP_DISCOVERY_RELAYS`
/// entry mapped to its `RelayRole`. Called from `Start` so the cold-start
/// kind:10002 discovery fetch has a socket to leave on before any NIP-65
/// list is cached. Per-author/recipient sockets spawn on demand in
/// `send_outbound` as the kernel emits OutboundMessages targeting their
/// resolved relay URLs.
pub(super) fn spawn_missing_relays(
    relay_controls: &mut HashMap<String, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
) {
    for role in RelayRole::all() {
        let bootstrap = role.bootstrap_url().to_string();
        ensure_relay_worker(
            relay_controls,
            relay_tx,
            kernel,
            next_relay_generation,
            role,
            bootstrap,
        );
    }
}

/// Spawn (if missing) a worker for `(role, relay_url)` and stamp the kernel's
/// per-role health row as `connecting`. Returns true iff a new worker was
/// spawned (the URL was previously unseen). On-demand path: any
/// `OutboundMessage` carrying a URL the pool has never seen gets a fresh
/// socket here before `send_outbound` enqueues the frame.
pub(super) fn ensure_relay_worker(
    relay_controls: &mut HashMap<String, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
    role: RelayRole,
    relay_url: String,
) -> bool {
    if relay_controls.contains_key(&relay_url) {
        return false;
    }
    let generation = *next_relay_generation;
    *next_relay_generation = generation.saturating_add(1);
    kernel.relay_connecting(role);
    relay_controls.insert(
        relay_url.clone(),
        RelayControl {
            generation,
            role,
            relay_url: relay_url.clone(),
            tx: spawn_relay_worker(role, relay_url, generation, relay_tx.clone()),
        },
    );
    true
}

pub(super) fn maybe_send_startup(
    running: bool,
    startup_sent: &mut bool,
    connected_relays: &HashSet<RelayRole>,
    relay_controls: &mut HashMap<String, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
) -> bool {
    if !running || *startup_sent || !all_relays_connected(connected_relays) {
        return false;
    }

    let startup_requests = kernel.startup_requests();
    send_all_outbound(
        relay_controls,
        relay_tx,
        kernel,
        next_relay_generation,
        startup_requests,
    );
    let view_requests = kernel.pending_view_requests();
    send_all_outbound(
        relay_controls,
        relay_tx,
        kernel,
        next_relay_generation,
        view_requests,
    );
    *startup_sent = true;
    true
}

pub(super) fn send_all_outbound(
    relay_controls: &mut HashMap<String, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
    outbound: Vec<OutboundMessage>,
) {
    // M5+M2+M8 wiring: every outbound batch passes through the AUTH-pause
    // partition before hitting the wire. REQs targeting an AUTH-paused
    // relay (ChallengeReceived / Authenticating) are diverted into the
    // deferred queue and replayed on the next tick after Authenticated.
    let outbound = kernel.partition_auth_paused(outbound);
    for message in outbound {
        send_outbound(
            relay_controls,
            relay_tx,
            kernel,
            next_relay_generation,
            message,
        );
    }
}

/// Route one `OutboundMessage` to the worker for its `relay_url`. Spawns a
/// new worker on first sight (per-URL on-demand). The previous role-based
/// fallback (defer when role's socket is missing) is gone — every message
/// resolves a concrete URL now (T105).
pub(super) fn send_outbound(
    relay_controls: &mut HashMap<String, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
    message: OutboundMessage,
) {
    // Spawn on demand for any URL the pool has not seen before. The
    // diagnostic lane is `message.role`; the actual socket dials `relay_url`.
    let _spawned = ensure_relay_worker(
        relay_controls,
        relay_tx,
        kernel,
        next_relay_generation,
        message.role,
        message.relay_url.clone(),
    );

    let Some(control) = relay_controls.get(&message.relay_url) else {
        // ensure_relay_worker only fails to insert under a logic bug — defer
        // so the frame isn't dropped silently.
        kernel.defer_outbound(message);
        return;
    };

    kernel.record_tx(message.role, message.text.len());
    if control.tx.send(RelayCommand::Send(message.text)).is_err() {
        kernel.relay_failed(message.role, "relay worker stopped".to_string());
    }
}

/// Shut down the worker for `url` (if one exists) and remove it from the pool.
///
/// Mirrors `ensure_relay_worker` in the remove direction. Sends
/// `RelayCommand::Shutdown` to the worker, which causes the worker thread to
/// close the socket and emit `RelayEvent::Closed` back to the actor loop.
/// The `relay_controls` entry is dropped immediately so the URL is no longer
/// in the pool — future `ensure_relay_worker` calls for the same URL will
/// spawn a fresh worker (T126 invariant preserved).
///
/// Returns `true` if a worker was found and shut down, `false` if the URL was
/// not in the pool (idempotent, no panic).
pub(super) fn shutdown_relay_worker(
    relay_controls: &mut HashMap<String, RelayControl>,
    url: &str,
) -> bool {
    let Some(control) = relay_controls.remove(url) else {
        return false;
    };
    // Best-effort send: if the worker channel is already closed the worker
    // has already exited — treat as success (the entry is gone from the pool).
    let _ = control.tx.send(RelayCommand::Shutdown);
    true
}

pub(super) fn close_relays(
    relay_controls: &mut HashMap<String, RelayControl>,
    connected_relays: &mut HashSet<RelayRole>,
    kernel: &mut Kernel,
) {
    // Close every active wire-sub on every per-URL socket. The kernel's
    // `active_subscriptions(role)` enumerates WireSubs by lane — we route
    // each CLOSE to the socket the sub was opened on (URL recorded in
    // WireSub by req_for_relay).
    let active = kernel.snapshot_active_wire_subs();
    for (sub_id, relay_url) in active {
        if let Some(control) = relay_controls.get(&relay_url) {
            let close = json!(["CLOSE", sub_id]).to_string();
            let _ = control.tx.send(RelayCommand::Send(close));
        }
    }
    for (_url, control) in relay_controls.drain() {
        let _ = control.tx.send(RelayCommand::Shutdown);
    }
    // Mirror the lane-level "closed" status into the kernel diagnostics.
    let _ = bootstrap_lane_close(connected_relays, kernel);
}

/// Mark each lane as closed once all its sockets are gone (post-drain).
fn bootstrap_lane_close(
    connected_relays: &mut HashSet<RelayRole>,
    kernel: &mut Kernel,
) -> [(); 0] {
    for role in RelayRole::all() {
        connected_relays.remove(&role);
        kernel.relay_closed(role);
    }
    // Ensure cold-start bootstrap seeds re-appear in the next Start cycle.
    let _ = BOOTSTRAP_DISCOVERY_RELAYS;
    []
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T126 — one-socket-per-URL invariant.
    ///
    /// Two `ensure_relay_worker` calls for the same byte-identical URL across
    /// different `RelayRole` lanes must yield exactly one `RelayControl` in the
    /// pool. The second call must return `false` (no new worker spawned) and
    /// the existing `tx` must be retained. `role` is a diagnostic-lane label
    /// only; it MUST NOT participate in pool keying.
    ///
    /// Worker threads spawned here will fail DNS / TCP-connect against
    /// `wss://127.0.0.1:1/` (port 1 — connection refused on all hosts) and
    /// exit; we test the synchronous keying decision in `ensure_relay_worker`.
    #[test]
    fn same_url_two_roles_yields_one_control() {
        let mut kernel = Kernel::new(80);
        let (relay_tx, _relay_rx) = std::sync::mpsc::channel::<RelayEvent>();
        let mut relay_controls: HashMap<String, RelayControl> = HashMap::new();
        let mut next_relay_generation = 1_u64;
        let url = "wss://127.0.0.1:1/".to_string();

        let spawned_a = ensure_relay_worker(
            &mut relay_controls,
            &relay_tx,
            &mut kernel,
            &mut next_relay_generation,
            RelayRole::Content,
            url.clone(),
        );
        let spawned_b = ensure_relay_worker(
            &mut relay_controls,
            &relay_tx,
            &mut kernel,
            &mut next_relay_generation,
            RelayRole::Indexer,
            url.clone(),
        );

        assert!(spawned_a, "first call must spawn");
        assert!(!spawned_b, "second call MUST short-circuit on URL match");
        assert_eq!(
            relay_controls.len(),
            1,
            "T126: one socket per URL — got {} entries",
            relay_controls.len()
        );
        let control = relay_controls.get(&url).expect("entry must exist");
        assert_eq!(
            control.role,
            RelayRole::Content,
            "role field is set at first insert and not rebound on subsequent ensure calls"
        );

        // Cleanly drain workers so they don't outlive the test.
        let mut connected = HashSet::new();
        close_relays(&mut relay_controls, &mut connected, &mut kernel);
    }

    /// T126 — three-role coverage including the post-`2afa4b1` Wallet lane.
    ///
    /// Locks in that the NWC wallet relay path does NOT bypass URL-keyed
    /// dedup: a wallet URL that collides with a content/indexer URL shares
    /// one socket. This is the future-proof case the invariant doc §1
    /// requires ("RoutingSource / RelayRole / … are aggregations over URLs,
    /// never multiplexing keys").
    #[test]
    fn same_url_three_roles_including_wallet_yields_one_control() {
        let mut kernel = Kernel::new(80);
        let (relay_tx, _relay_rx) = std::sync::mpsc::channel::<RelayEvent>();
        let mut relay_controls: HashMap<String, RelayControl> = HashMap::new();
        let mut next_relay_generation = 1_u64;
        let url = "wss://127.0.0.1:1/".to_string();

        for role in [RelayRole::Content, RelayRole::Indexer, RelayRole::Wallet] {
            let _ = ensure_relay_worker(
                &mut relay_controls,
                &relay_tx,
                &mut kernel,
                &mut next_relay_generation,
                role,
                url.clone(),
            );
        }

        assert_eq!(
            relay_controls.len(),
            1,
            "T126: one socket per URL across Content+Indexer+Wallet — got {}",
            relay_controls.len()
        );

        let mut connected = HashSet::new();
        close_relays(&mut relay_controls, &mut connected, &mut kernel);
    }

    /// T162 — `shutdown_relay_worker` removes the worker entry from `relay_controls`.
    ///
    /// Verifies that after `ensure_relay_worker` dials a loopback socket and emits
    /// `Connected`, calling `shutdown_relay_worker` with the same URL sends
    /// `RelayCommand::Shutdown` to the worker and removes the entry from
    /// `relay_controls`. The T126 invariant is preserved: after shutdown, the
    /// URL is no longer in the pool.
    ///
    /// Uses `ws://` (plain TCP) to avoid TLS setup overhead in unit tests.
    #[test]
    fn t_remove_relay_shuts_down_worker() {
        use super::shutdown_relay_worker;
        use std::net::TcpListener;
        use std::sync::mpsc::{self, RecvTimeoutError};
        use std::thread;
        use std::time::Duration;

        // Bind a loopback listener; port 0 → OS picks a free port.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
        let port = listener.local_addr().expect("local_addr").port();
        let relay_url = format!("ws://127.0.0.1:{port}");

        // Minimal server: accept one connection, complete the WS handshake, park.
        let _server = thread::spawn(move || {
            listener.set_nonblocking(false).ok();
            let (stream, _) = match listener.accept() {
                Ok(s) => s,
                Err(_) => return,
            };
            stream.set_read_timeout(Some(Duration::from_millis(50))).ok();
            let mut socket = match tungstenite::accept(stream) {
                Ok(s) => s,
                Err(_) => return,
            };
            // Drain frames until the connection closes (worker shutdown).
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                match socket.read() {
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(e))
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(_) => return,
                }
            }
        });
        // Give the server thread a moment to enter `accept()` before the worker dials.
        thread::sleep(Duration::from_millis(30));

        let mut kernel = Kernel::new(80);
        let (relay_tx, relay_rx) = mpsc::channel::<RelayEvent>();
        let mut relay_controls: HashMap<String, RelayControl> = HashMap::new();
        let mut next_gen = 1_u64;

        // Step 1: add relay, wait for Connected.
        let spawned = ensure_relay_worker(
            &mut relay_controls,
            &relay_tx,
            &mut kernel,
            &mut next_gen,
            RelayRole::Content,
            relay_url.clone(),
        );
        assert!(spawned, "first ensure_relay_worker call must spawn a worker");
        assert_eq!(relay_controls.len(), 1, "pool must have exactly one entry after add");

        // Wait for Connected event.
        let mut got_connected = false;
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            match relay_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(RelayEvent::Connected { relay_url: url, .. }) if url == relay_url => {
                    got_connected = true;
                    break;
                }
                Ok(_) => continue,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        assert!(
            got_connected,
            "T162: ensure_relay_worker must emit Connected within 3s (url={relay_url})"
        );

        // Step 2: shutdown — this is the fix we are RED-testing. Before the fix
        // shutdown_relay_worker does not exist; this call will not compile until
        // the GREEN phase adds the function.
        let removed = shutdown_relay_worker(&mut relay_controls, &relay_url);
        assert!(removed, "T162: shutdown_relay_worker must return true for a known URL");
        assert!(
            !relay_controls.contains_key(&relay_url),
            "T162: relay_controls must NOT contain the URL after RemoveRelay shutdown"
        );
    }

    /// T162 — `shutdown_relay_worker` for an unknown URL is a no-op (no panic).
    ///
    /// `RemoveRelay` for a URL that was never added (no worker in the pool)
    /// must be idempotent — it must not panic, must return false, and must
    /// leave `relay_controls` empty.
    #[test]
    fn t_remove_relay_unknown_url_is_noop() {
        use super::shutdown_relay_worker;

        let mut relay_controls: HashMap<String, RelayControl> = HashMap::new();
        let url = "wss://nonexistent.example.com/".to_string();

        let removed = shutdown_relay_worker(&mut relay_controls, &url);
        assert!(
            !removed,
            "T162: shutdown_relay_worker for unknown URL must return false"
        );
        assert!(
            relay_controls.is_empty(),
            "T162: relay_controls must remain empty after noop shutdown"
        );
    }

    /// T158 — `ensure_relay_worker` dials a real loopback socket and emits `Connected`.
    ///
    /// This is the component-level proof that the `AddRelay` dispatch arm (T158
    /// fix) calls `ensure_relay_worker` with the user-supplied URL, which in turn
    /// spawns a worker that completes the WebSocket handshake and emits
    /// `RelayEvent::Connected`.
    ///
    /// Uses `ws://` (plain TCP) to avoid TLS setup overhead in unit tests.
    /// The actor-level integration (command → dispatch → ensure_relay_worker) is
    /// proven by compilation + the `commands::add_relay` return-value tests; this
    /// test pins the socket-dial behaviour of the underlying primitive.
    #[test]
    fn t158_ensure_relay_worker_dials_and_emits_connected() {
        use std::net::TcpListener;
        use std::sync::mpsc::{self, RecvTimeoutError};
        use std::thread;
        use std::time::Duration;

        // Bind a loopback listener; port 0 → OS picks a free port.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
        let port = listener.local_addr().expect("local_addr").port();
        let relay_url = format!("ws://127.0.0.1:{port}");

        // Minimal server: accept one connection, complete the WS handshake, park.
        let _server = thread::spawn(move || {
            listener.set_nonblocking(false).ok();
            let (stream, _) = match listener.accept() {
                Ok(s) => s,
                Err(_) => return,
            };
            stream.set_read_timeout(Some(Duration::from_millis(50))).ok();
            let mut socket = match tungstenite::accept(stream) {
                Ok(s) => s,
                Err(_) => return,
            };
            // Drain frames until the test tears down the connection.
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                match socket.read() {
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(e))
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(_) => return,
                }
            }
        });
        // Give the server thread a moment to enter `accept()` before the worker dials.
        thread::sleep(Duration::from_millis(30));

        let mut kernel = Kernel::new(80);
        let (relay_tx, relay_rx) = mpsc::channel::<RelayEvent>();
        let mut relay_controls: HashMap<String, RelayControl> = HashMap::new();
        let mut next_gen = 1_u64;

        let spawned = ensure_relay_worker(
            &mut relay_controls,
            &relay_tx,
            &mut kernel,
            &mut next_gen,
            RelayRole::Content,
            relay_url.clone(),
        );
        assert!(spawned, "first ensure_relay_worker call must spawn a worker");
        assert_eq!(relay_controls.len(), 1, "pool must have exactly one entry");

        // Wait for the Connected event — proves the socket actually dialled.
        let mut got_connected = false;
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            match relay_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(RelayEvent::Connected { relay_url: url, .. }) if url == relay_url => {
                    got_connected = true;
                    break;
                }
                Ok(_) => continue,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        assert!(
            got_connected,
            "T158: ensure_relay_worker must emit Connected for the user-added relay \
             within 3s (url={relay_url})"
        );

        close_relays(&mut relay_controls, &mut HashSet::new(), &mut kernel);
    }
}
