use super::*;
use nmp_network::pool::{Pool, PoolConfig, PoolEvent};

/// Build the actor-side transport state every test in this file needs: a
/// fresh [`Pool`] (phase F's substrate for per-URL workers), the event
/// receiver (kept around so the channel doesn't disconnect mid-test), the
/// empty `relay_controls` map, and the `slot_to_url` reverse-lookup
/// side-map. Returns `(pool, events_rx, relay_controls, slot_to_url)`.
fn fresh_pool() -> (
    Pool,
    std::sync::mpsc::Receiver<PoolEvent>,
    HashMap<CanonicalRelayUrl, RelayControl>,
    HashMap<u32, CanonicalRelayUrl>,
) {
    let (events_tx, events_rx) = std::sync::mpsc::channel::<PoolEvent>();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    (pool, events_rx, HashMap::new(), HashMap::new())
}

/// T126 — one-socket-per-URL invariant.
///
/// Two `ensure_relay_worker` calls for the same byte-identical URL across
/// different `RelayRole` lanes must yield exactly one `RelayControl` in the
/// pool. The second call must return `false` (no new worker spawned) and
/// the existing handle must be retained. `role` is a diagnostic-lane label
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
    let (pool, _events_rx, mut relay_controls, mut slot_to_url) = fresh_pool();
    let mut next_relay_generation = 1_u64;
    // Supply with trailing slash — canonical form strips it.
    let raw_url = "wss://127.0.0.1:1/".to_string();
    let canonical_key = "wss://127.0.0.1:1"; // expected pool key after canonicalization

    let spawned_a = ensure_relay_worker(
        &mut relay_controls,
        &mut slot_to_url,
        &pool,
        &mut kernel,
        &mut next_relay_generation,
        RelayRole::Content,
        raw_url.clone(),
    );
    let spawned_b = ensure_relay_worker(
        &mut relay_controls,
        &mut slot_to_url,
        &pool,
        &mut kernel,
        &mut next_relay_generation,
        RelayRole::Indexer,
        raw_url.clone(),
    );

    assert!(spawned_a, "first call must spawn");
    assert!(
        !spawned_b,
        "second call MUST short-circuit on canonical URL match"
    );
    assert_eq!(
        relay_controls.len(),
        1,
        "T126: one socket per URL — got {} entries",
        relay_controls.len()
    );
    // Pool key is the canonical form (no trailing slash), not the raw input.
    let control = relay_controls
        .get(&CanonicalRelayUrl::parse_or_raw(canonical_key))
        .expect("entry must exist under canonical key");
    assert_eq!(
        control.role,
        RelayRole::Content,
        "role field is set at first insert and not rebound on subsequent ensure calls"
    );

    // Cleanly drain workers so they don't outlive the test.
    let mut connected = HashSet::new();
    close_relays(
        &mut relay_controls,
        &mut slot_to_url,
        &pool,
        &mut connected,
        &mut kernel,
    );
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
    let (pool, _events_rx, mut relay_controls, mut slot_to_url) = fresh_pool();
    let mut next_relay_generation = 1_u64;
    let url = "wss://127.0.0.1:1/".to_string();

    for role in [RelayRole::Content, RelayRole::Indexer, RelayRole::Wallet] {
        let _ = ensure_relay_worker(
            &mut relay_controls,
            &mut slot_to_url,
            &pool,
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
    close_relays(
        &mut relay_controls,
        &mut slot_to_url,
        &pool,
        &mut connected,
        &mut kernel,
    );
}

/// T162 — `shutdown_relay_worker` removes the worker entry from `relay_controls`.
///
/// Verifies that after `ensure_relay_worker` dials a loopback socket and emits
/// `PoolEvent::Opened`, calling `shutdown_relay_worker` with the same URL
/// closes the pool slot and removes the entry from `relay_controls`. The
/// T126 invariant is preserved: after shutdown, the URL is no longer in the
/// pool.
///
/// Uses `ws://` (plain TCP) to avoid TLS setup overhead in unit tests.
#[test]
fn t_remove_relay_shuts_down_worker() {
    use super::shutdown_relay_worker;
    use std::net::TcpListener;
    use std::sync::mpsc::RecvTimeoutError;
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
        stream
            .set_read_timeout(Some(Duration::from_millis(50)))
            .ok();
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
    let (pool, events_rx, mut relay_controls, mut slot_to_url) = fresh_pool();
    let mut next_gen = 1_u64;

    // Step 1: add relay, wait for Opened (pool-side rename of legacy Connected).
    let spawned = ensure_relay_worker(
        &mut relay_controls,
        &mut slot_to_url,
        &pool,
        &mut kernel,
        &mut next_gen,
        RelayRole::Content,
        relay_url.clone(),
    );
    assert!(
        spawned,
        "first ensure_relay_worker call must spawn a worker"
    );
    assert_eq!(
        relay_controls.len(),
        1,
        "pool must have exactly one entry after add"
    );

    // Wait for the Opened event (PoolEvent variant for "socket dial completed").
    let mut got_opened = false;
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        match events_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(PoolEvent::Opened { url, .. }) if url == relay_url => {
                got_opened = true;
                break;
            }
            Ok(_) => continue,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    assert!(
        got_opened,
        "T162: ensure_relay_worker must emit Opened within 3s (url={relay_url})"
    );

    // Step 2: shutdown — closes the pool slot and drops the control row.
    let removed = shutdown_relay_worker(&mut relay_controls, &mut slot_to_url, &pool, &relay_url);
    assert!(
        removed,
        "T162: shutdown_relay_worker must return true for a known URL"
    );
    assert!(
        !relay_controls.contains_key(&CanonicalRelayUrl::parse_or_raw(&relay_url)),
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

    let (pool, _events_rx, mut relay_controls, mut slot_to_url) = fresh_pool();
    let url = "wss://nonexistent.example.com/".to_string();

    let removed = shutdown_relay_worker(&mut relay_controls, &mut slot_to_url, &pool, &url);
    assert!(
        !removed,
        "T162: shutdown_relay_worker for unknown URL must return false"
    );
    assert!(
        relay_controls.is_empty(),
        "T162: relay_controls must remain empty after noop shutdown"
    );
}

/// T158 — `ensure_relay_worker` dials a real loopback socket and emits `Opened`.
///
/// This is the component-level proof that the `AddRelay` dispatch arm (T158
/// fix) calls `ensure_relay_worker` with the user-supplied URL, which in turn
/// spawns a pool worker that completes the WebSocket handshake and emits
/// `PoolEvent::Opened`.
///
/// Uses `ws://` (plain TCP) to avoid TLS setup overhead in unit tests.
/// The actor-level integration (command → dispatch → ensure_relay_worker) is
/// proven by compilation + the `commands::add_relay` return-value tests; this
/// test pins the socket-dial behaviour of the underlying primitive.
#[test]
fn t158_ensure_relay_worker_dials_and_emits_connected() {
    use std::net::TcpListener;
    use std::sync::mpsc::RecvTimeoutError;
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
        stream
            .set_read_timeout(Some(Duration::from_millis(50)))
            .ok();
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
    let (pool, events_rx, mut relay_controls, mut slot_to_url) = fresh_pool();
    let mut next_gen = 1_u64;

    let spawned = ensure_relay_worker(
        &mut relay_controls,
        &mut slot_to_url,
        &pool,
        &mut kernel,
        &mut next_gen,
        RelayRole::Content,
        relay_url.clone(),
    );
    assert!(
        spawned,
        "first ensure_relay_worker call must spawn a worker"
    );
    assert_eq!(relay_controls.len(), 1, "pool must have exactly one entry");

    // Wait for the Opened event — proves the socket actually dialled.
    let mut got_opened = false;
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        match events_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(PoolEvent::Opened { url, .. }) if url == relay_url => {
                got_opened = true;
                break;
            }
            Ok(_) => continue,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    assert!(
        got_opened,
        "T158: ensure_relay_worker must emit Opened for the user-added relay \
         within 3s (url={relay_url})"
    );

    close_relays(
        &mut relay_controls,
        &mut slot_to_url,
        &pool,
        &mut HashSet::new(),
        &mut kernel,
    );
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
    use std::sync::mpsc::RecvTimeoutError;
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
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .ok();
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
    let (pool, events_rx, mut relay_controls, mut slot_to_url) = fresh_pool();
    let mut next_gen = 1_u64;

    // Pre-add via canonical URL so the worker is in the pool.
    let spawned = ensure_relay_worker(
        &mut relay_controls,
        &mut slot_to_url,
        &pool,
        &mut kernel,
        &mut next_gen,
        RelayRole::Content,
        canonical_url.clone(),
    );
    assert!(spawned, "initial ensure must spawn");

    // Wait for Opened before sending.
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        match events_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(PoolEvent::Opened { .. }) => break,
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
        &mut slot_to_url,
        &pool,
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

    close_relays(
        &mut relay_controls,
        &mut slot_to_url,
        &pool,
        &mut HashSet::new(),
        &mut kernel,
    );
}
