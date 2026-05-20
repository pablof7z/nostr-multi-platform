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
use crate::relay::{canonical_relay_url, OutboundMessage, RelayRole};
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

/// Lane-bootstrap seeds: spawn one worker per configured URL returned by
/// `kernel.bootstrap_urls_for_role(role)`. Called from `Start` so the cold-start
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
        for url in kernel.bootstrap_urls_for_role(role) {
            ensure_relay_worker(
                relay_controls,
                relay_tx,
                kernel,
                next_relay_generation,
                role,
                url,
            );
        }
    }
}

/// Spawn (if missing) a worker for `(role, relay_url)` and stamp the kernel's
/// per-role health row as `connecting`. Returns true iff a new worker was
/// spawned (the URL was previously unseen). On-demand path: any
/// `OutboundMessage` carrying a URL the pool has never seen gets a fresh
/// socket here before `send_outbound` enqueues the frame.
///
/// T-relay-url-normalize: `relay_url` is passed through
/// [`canonical_relay_url`] before the pool-key lookup so that URL-equivalent
/// forms (differing only in case, trailing-slash-on-empty-path, or leading
/// whitespace) all resolve to the same pool entry. If the URL cannot be
/// canonicalized (e.g. a bootstrap seed that is already lowercase+clean), the
/// original string is used unchanged — existing bootstrap behaviour is
/// preserved.
pub(super) fn ensure_relay_worker(
    relay_controls: &mut HashMap<String, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
    role: RelayRole,
    relay_url: String,
) -> bool {
    // Canonicalize the URL so all callers (add, send_outbound, bootstrap)
    // agree on the pool key. Fall back to the raw string for URLs that don't
    // parse as ws/wss (e.g. bootstrap seeds that are already canonical).
    let key = canonical_relay_url(&relay_url).unwrap_or_else(|| relay_url.clone());
    if relay_controls.contains_key(&key) {
        return false;
    }
    let generation = *next_relay_generation;
    *next_relay_generation = generation.saturating_add(1);
    kernel.relay_connecting(role);
    relay_controls.insert(
        key.clone(),
        RelayControl {
            generation,
            role,
            relay_url: key.clone(),
            tx: spawn_relay_worker(role, key, generation, relay_tx.clone()),
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
///
/// T-relay-url-normalize: both the spawn call and the subsequent pool lookup
/// must use the same canonical key. `ensure_relay_worker` canonicalizes
/// internally and stores the canonical key, so the `relay_controls.get()`
/// must also use the canonical form — otherwise a non-canonical
/// `message.relay_url` (trailing slash / uppercase scheme) would miss the
/// entry and silently defer the frame forever.
pub(super) fn send_outbound(
    relay_controls: &mut HashMap<String, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
    message: OutboundMessage,
) {
    // Resolve to the canonical pool key first so both the spawn and the
    // subsequent lookup agree on the same HashMap entry.
    let canonical_key = canonical_relay_url(&message.relay_url)
        .unwrap_or_else(|| message.relay_url.clone());

    // Spawn on demand for any URL the pool has not seen before. The
    // diagnostic lane is `message.role`; the actual socket dials `canonical_key`.
    let _spawned = ensure_relay_worker(
        relay_controls,
        relay_tx,
        kernel,
        next_relay_generation,
        message.role,
        canonical_key.clone(),
    );

    let Some(control) = relay_controls.get(&canonical_key) else {
        // ensure_relay_worker only fails to insert under a logic bug — defer
        // so the frame isn't dropped silently.
        kernel.defer_outbound(message);
        return;
    };

    kernel.record_tx(message.role, message.text.len());
    if control.tx.send(RelayCommand::Send(message.text)).is_err() {
        // T105: the dead channel is this specific socket — scope the
        // `retrying` mark to its URL, not the whole role lane.
        kernel.relay_failed(message.role, &canonical_key, "relay worker stopped".to_string());
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
/// T-relay-url-normalize: `url` is canonicalized before the pool-key lookup so
/// that removing `"wss://R.Ex/"` correctly finds the entry stored under the
/// canonical key `"wss://r.ex"`. If the URL cannot be canonicalized, the raw
/// string is tried as-is (idempotent, no panic).
///
/// Returns `true` if a worker was found and shut down, `false` if the URL was
/// not in the pool (idempotent, no panic).
pub(super) fn shutdown_relay_worker(
    relay_controls: &mut HashMap<String, RelayControl>,
    url: &str,
) -> bool {
    let key = canonical_relay_url(url).unwrap_or_else(|| url.to_string());
    let Some(control) = relay_controls.remove(&key) else {
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
        // T-relay-url-normalize: wire-sub URLs may carry non-canonical forms
        // (trailing slash, uppercase scheme) — canonicalize before pool lookup
        // so the CLOSE frame reaches the correct worker.
        let key = canonical_relay_url(&relay_url).unwrap_or_else(|| relay_url.clone());
        if let Some(control) = relay_controls.get(&key) {
            let close = json!(["CLOSE", sub_id]).to_string();
            let _ = control.tx.send(RelayCommand::Send(close));
        }
    }
    for (_url, control) in relay_controls.drain() {
        let _ = control.tx.send(RelayCommand::Shutdown);
    }
    // Mirror the lane-level "closed" status into the kernel diagnostics.
    bootstrap_lane_close(connected_relays, kernel);
}

/// Mark each lane as closed once all its sockets are gone (post-drain).
fn bootstrap_lane_close(
    connected_relays: &mut HashSet<RelayRole>,
    kernel: &mut Kernel,
) {
    for role in RelayRole::all() {
        connected_relays.remove(&role);
        // Global teardown: every socket of every role is being drained, so
        // evict the whole lane (the per-URL `relay_closed` would force the
        // caller to enumerate sockets it is discarding anyway — T105).
        kernel.relay_closed_all(role);
    }
    // Cold-start bootstrap seeds will be respawned from relay_edit_rows on the next Start cycle.
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
    /// T-relay-url-normalize: `ensure_relay_worker` now canonicalizes the URL
    /// before the pool lookup. The canonical key for `wss://127.0.0.1:1/` is
    /// `wss://127.0.0.1:1` (empty-path trailing slash stripped). This test
    /// uses the canonical form for the pool-key lookup so it reflects the
    /// correct post-normalization behaviour.
    ///
    /// Worker threads spawned here will fail DNS / TCP-connect against
    /// `wss://127.0.0.1:1` (port 1 — connection refused on all hosts) and
    /// exit; we test the synchronous keying decision in `ensure_relay_worker`.
    #[test]
    fn same_url_two_roles_yields_one_control() {
        let mut kernel = Kernel::new(80);
        let (relay_tx, _relay_rx) = std::sync::mpsc::channel::<RelayEvent>();
        let mut relay_controls: HashMap<String, RelayControl> = HashMap::new();
        let mut next_relay_generation = 1_u64;
        // Supply with trailing slash — canonical form strips it.
        let raw_url = "wss://127.0.0.1:1/".to_string();
        let canonical_key = "wss://127.0.0.1:1"; // expected pool key after canonicalization

        let spawned_a = ensure_relay_worker(
            &mut relay_controls,
            &relay_tx,
            &mut kernel,
            &mut next_relay_generation,
            RelayRole::Content,
            raw_url.clone(),
        );
        let spawned_b = ensure_relay_worker(
            &mut relay_controls,
            &relay_tx,
            &mut kernel,
            &mut next_relay_generation,
            RelayRole::Indexer,
            raw_url.clone(),
        );

        assert!(spawned_a, "first call must spawn");
        assert!(!spawned_b, "second call MUST short-circuit on canonical URL match");
        assert_eq!(
            relay_controls.len(),
            1,
            "T126: one socket per URL — got {} entries",
            relay_controls.len()
        );
        // Pool key is the canonical form (no trailing slash), not the raw input.
        let control = relay_controls.get(canonical_key).expect("entry must exist under canonical key");
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

    /// T-normalize-send-outbound: `send_outbound` with a non-canonical URL
    /// (trailing slash / uppercase) must NOT defer the frame.
    ///
    /// Regression: the previous `d636a6b` commit fixed `ensure_relay_worker`
    /// and `shutdown_relay_worker` to canonicalize, but left `send_outbound`
    /// looking up by raw `message.relay_url` after `ensure_relay_worker` had
    /// stored the canonical key — causing every frame destined for a URL with
    /// a trailing slash to be silently deferred forever.
    ///
    /// This test calls `send_outbound` with a trailing-slash URL whose
    /// canonical worker is already in the pool and asserts:
    ///   1. Pool count stays at 1 (no duplicate socket spawned).
    ///   2. `kernel.deferred_outbound_len()` is 0 (frame was routed, not deferred).
    #[test]
    fn t_normalize_send_outbound_non_canonical_url_routes_not_deferred() {
        use crate::relay::OutboundMessage;
        use std::net::TcpListener;
        use std::sync::mpsc::{self, RecvTimeoutError};
        use std::thread;
        use std::time::Duration;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
        let port = listener.local_addr().expect("local_addr").port();
        // canonical = no trailing slash
        let canonical_url = format!("ws://127.0.0.1:{port}");
        // non-canonical = trailing slash variant that used to cause a lookup miss
        let non_canonical_url = format!("ws://127.0.0.1:{port}/");

        let _server = thread::spawn(move || {
            listener.set_nonblocking(false).ok();
            let (stream, _) = match listener.accept() {
                Ok(s) => s,
                Err(_) => return,
            };
            stream.set_read_timeout(Some(Duration::from_millis(100))).ok();
            let mut socket = match tungstenite::accept(stream) {
                Ok(s) => s,
                Err(_) => return,
            };
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
        thread::sleep(Duration::from_millis(30));

        let mut kernel = Kernel::new(80);
        let (relay_tx, relay_rx) = mpsc::channel::<RelayEvent>();
        let mut relay_controls: HashMap<String, RelayControl> = HashMap::new();
        let mut next_gen = 1_u64;

        // Pre-add via canonical URL so the worker is in the pool.
        let spawned = ensure_relay_worker(
            &mut relay_controls,
            &relay_tx,
            &mut kernel,
            &mut next_gen,
            RelayRole::Content,
            canonical_url.clone(),
        );
        assert!(spawned, "initial ensure must spawn");

        // Wait for Connected before sending.
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            match relay_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(RelayEvent::Connected { .. }) => break,
                Ok(_) | Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        // Send via the non-canonical (trailing-slash) form. The fix must route
        // this to the existing worker, not defer it.
        let msg = OutboundMessage {
            role: RelayRole::Content,
            relay_url: non_canonical_url.clone(),
            text: r#"["REQ","t-normalize-sub",{"kinds":[1],"limit":1}]"#.to_string(),
        };
        send_outbound(
            &mut relay_controls,
            &relay_tx,
            &mut kernel,
            &mut next_gen,
            msg,
        );

        // Pool must still have exactly one entry (no duplicate spawned).
        assert_eq!(
            relay_controls.len(),
            1,
            "T-normalize-send-outbound: pool must have 1 entry after send_outbound \
             with non-canonical URL (trailing slash), got {}",
            relay_controls.len()
        );

        // Deferred queue must be empty — the frame was routed, not deferred.
        assert_eq!(
            kernel.deferred_outbound_len(),
            0,
            "T-normalize-send-outbound: deferred queue must be empty — \
             frame with non-canonical URL must NOT be deferred"
        );

        close_relays(&mut relay_controls, &mut HashSet::new(), &mut kernel);
    }
}
