//! Bounded worker pool for the per-relay REQ fan-out, plus the synchronous
//! discovery-probe fan used by the `req` tick loop.
//!
//! The content fanout sends the lifecycle's `WireFrame::Req.filter_json`
//! **verbatim** — the REPL no longer rebuilds the filter from the AST. The
//! relays + filters come from the production lifecycle's compiled plan
//! (post-`apply_selection`), not a manual compiler call.
//!
//! Pitfall §13.5 — workers are detached at the wall deadline; no graceful
//! cancellation. Sockets drop on scope exit; OS reclaims them.
//!
//! ### Finding: probe subs are untracked by the lifecycle
//!
//! Implicit kind:10002 discovery REQs are appended in `recompile_and_diff`
//! *after* `auth_gate.partition` and `lifecycle_gate.observe_diff`, and are
//! NOT inserted into `current_plan`. The lifecycle therefore never emits a
//! CLOSE for a probe sub. In production the actor lets the indexer socket
//! drop; here `run_discovery` CLOSEs each probe sub client-side after EOSE
//! so we don't leak an open kind:10002 sub. See the final report.

use std::collections::BTreeMap;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::Message;

use crate::discovery::parse_kind10002;
use crate::ws::{next_frame, summarize_filter, try_connect_msg, Frame};
use nmp_core::planner::MailboxSnapshot;

const FANOUT_MAX_WORKERS: usize = 64;

/// Hard cap on a single discovery round (defensive — the indexer normally
/// EOSEs in well under a second). The task's "short discovery wall".
const DISCOVERY_WALL: Duration = Duration::from_secs(5);

#[derive(Default, Clone, Debug)]
pub struct RelayStats {
    pub events: u64,
    pub authors_in_req: usize,
    pub time_to_first: Option<Duration>,
    pub connected: bool,
    pub eose: bool,
    pub error: Option<String>,
    pub elapsed: Option<Duration>,
}

/// Events emitted from a worker to the main thread. Every variant carries
/// the wire `sub_id` so the renderer can show ONE row per REQ — the
/// lifecycle keys live subs by `(relay_url, sub_id)` and a relay may carry
/// more than one sub shape, so a relay-only key would hide REQs.
#[derive(Debug)]
pub enum RelayEvent {
    Connecting { relay: String, sub_id: String },
    ReqSent { relay: String, sub_id: String },
    Frame {
        relay: String,
        sub_id: String,
        event_id: String,
    },
    Eose {
        relay: String,
        sub_id: String,
        elapsed: Duration,
    },
    /// Relay closed THIS sub (`CLOSED`) — terminal for the row. `msg` is the
    /// verbatim relay reason (e.g. `auth-required: rate limit exceeded`).
    Closed {
        relay: String,
        sub_id: String,
        msg: String,
    },
    /// Relay demanded NIP-42 AUTH. Read-only REPL: not authing. Terminal for
    /// every in-flight sub on this socket.
    Auth { relay: String, sub_id: String },
    /// Relay NOTICE. Non-terminal — surfaced but the row keeps streaming.
    Notice {
        relay: String,
        sub_id: String,
        msg: String,
    },
    /// Connect/IO failure or relay-initiated socket close — terminal.
    Error {
        relay: String,
        sub_id: String,
        msg: String,
    },
    Done {
        relay: String,
        sub_id: String,
        stats: RelayStats,
    },
}

/// One content REQ as produced by the lifecycle: the relay it targets, the
/// stable wire sub-id, the verbatim filter JSON, and the author count parsed
/// out of that filter (for the render row label).
#[derive(Clone, Debug)]
pub struct ContentReq {
    pub relay: String,
    pub sub_id: String,
    pub filter_json: String,
    pub authors: usize,
}

/// Synchronous discovery-probe fan. Sends each probe REQ (kind:10002, sub_id
/// prefix `mailbox-probe-`) verbatim to its indexer relay, drains kind:10002
/// EVENTs until every probe sub reaches EOSE or `DISCOVERY_WALL` elapses,
/// and returns the parsed snapshots.
///
/// `probes`: `(relay_url, sub_id, filter_json)` triples.
pub fn run_discovery(probes: &[(String, String, String)]) -> Vec<(String, MailboxSnapshot)> {
    let mut by_relay: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for (relay, sub_id, filter) in probes {
        by_relay
            .entry(relay.clone())
            .or_default()
            .push((sub_id.clone(), filter.clone()));
    }

    let n_subs: usize = by_relay.values().map(std::vec::Vec::len).sum();
    println!(
        "  discovery: {} implicit kind:10002 probe REQ{} across {} indexer{}",
        n_subs,
        if n_subs == 1 { "" } else { "s" },
        by_relay.len(),
        if by_relay.len() == 1 { "" } else { "s" }
    );

    let mut out: BTreeMap<String, MailboxSnapshot> = BTreeMap::new();
    for (relay, subs) in by_relay {
        println!("  connecting {relay} …");
        let mut socket = match try_connect_msg(&relay) {
            Ok(s) => s,
            Err(e) => {
                // Every probe sub on this relay failed to even open.
                for (sub_id, filter) in &subs {
                    println!(
                        "  {relay}  ✗ error: {e}  {} {sub_id}",
                        summarize_filter(filter)
                    );
                }
                continue;
            }
        };
        let mut open: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut events: std::collections::BTreeMap<String, u64> =
            std::collections::BTreeMap::new();
        for (sub_id, filter) in &subs {
            let raw_filter: Value =
                serde_json::from_str(filter).unwrap_or_else(|_| json!({ "kinds": [10002] }));
            let req = json!(["REQ", sub_id, raw_filter]).to_string();
            if socket.send(Message::Text(req)).is_ok() {
                open.insert(sub_id.clone());
                events.insert(sub_id.clone(), 0);
                println!(
                    "  {relay}  → REQ {} {sub_id}",
                    summarize_filter(filter)
                );
            } else {
                println!(
                    "  {relay}  ✗ error: send REQ  {} {sub_id}",
                    summarize_filter(filter)
                );
            }
        }
        let deadline = Instant::now() + DISCOVERY_WALL;
        // `auth_or_close` flags a socket-wide terminal (AUTH / relay close /
        // IO): every still-open probe sub on it gets that terminal line.
        let mut socket_terminal: Option<String> = None;
        while !open.is_empty() && Instant::now() < deadline {
            match next_frame(&mut socket) {
                Frame::Timeout | Frame::Other => continue,
                Frame::Event { sub_id, event } => {
                    if let Some(c) = events.get_mut(&sub_id) {
                        *c += 1;
                    }
                    if let Some((pk, snap)) = parse_kind10002(&event) {
                        out.insert(pk, snap);
                    }
                }
                Frame::Eose { sub_id } => {
                    if open.remove(&sub_id) {
                        println!(
                            "  {relay}  ✓ EOSE ({} event{}) {sub_id}",
                            events.get(&sub_id).copied().unwrap_or(0),
                            if events.get(&sub_id).copied().unwrap_or(0) == 1 {
                                ""
                            } else {
                                "s"
                            }
                        );
                    }
                }
                Frame::Closed { sub_id, message } => {
                    if open.remove(&sub_id) {
                        println!("  {relay}  ✗ CLOSED: {message}  {sub_id}");
                    }
                }
                Frame::Notice { message } => {
                    println!("  {relay}  • NOTICE: {message}");
                }
                Frame::Auth { .. } => {
                    socket_terminal = Some("AUTH required (read-only — not authing)".into());
                    break;
                }
                Frame::RelayClosed => {
                    socket_terminal = Some("connection closed by relay".into());
                    break;
                }
                Frame::Io { kind } => {
                    socket_terminal = Some(format!("error: {kind}"));
                    break;
                }
            }
        }
        // Any probe sub still open: name why (socket terminal, or timeout).
        for sub_id in &open {
            match &socket_terminal {
                Some(reason) => println!("  {relay}  ✗ {reason}  {sub_id}"),
                None => println!("  {relay}  ✗ timeout (no terminal frame)  {sub_id}"),
            }
        }
        // CLOSE every probe sub client-side: the lifecycle's wire-emitter
        // never tracked these (they are appended after auth-partition and
        // are not in `current_plan`), so nothing else will.
        for (sub_id, _) in &subs {
            let _ = socket.send(Message::Text(json!(["CLOSE", sub_id]).to_string()));
        }
        let _ = socket.close(None);
    }

    out.into_iter().collect()
}

/// Launch the content worker pool. One job per relay; each job carries that
/// relay's content REQs (already shaped by the lifecycle). Returns the event
/// receiver, the worker count, and the global wall deadline.
#[must_use] 
pub fn launch(
    per_relay: &BTreeMap<String, Vec<ContentReq>>,
    wall: Duration,
) -> (mpsc::Receiver<RelayEvent>, usize, Instant) {
    let (msg_tx, msg_rx) = mpsc::channel::<RelayEvent>();
    let (work_tx, work_rx) = mpsc::channel::<(String, Vec<ContentReq>)>();
    let work_rx = Arc::new(Mutex::new(work_rx));
    let global_deadline = Instant::now() + wall;

    let mut total_jobs = 0usize;
    for (relay_url, reqs) in per_relay {
        if !relay_url.starts_with("wss://") && !relay_url.starts_with("ws://") {
            continue;
        }
        // `work_rx` is held alive in the `Arc<Mutex<..>>` above for the whole
        // function, so this `send` cannot fail here. D2: `launch` is a public
        // API boundary — a disconnected channel is dropped silently (the job
        // simply isn't queued) rather than panicking the caller. `total_jobs`
        // is only incremented on a successful send so the worker count below
        // stays consistent with what was actually queued.
        if work_tx.send((relay_url.clone(), reqs.clone())).is_ok() {
            total_jobs += 1;
        }
    }
    drop(work_tx);

    let workers = FANOUT_MAX_WORKERS.min(total_jobs.max(1));
    for _ in 0..workers {
        let work_rx = work_rx.clone();
        let msg_tx = msg_tx.clone();
        thread::spawn(move || loop {
            if Instant::now() >= global_deadline {
                return;
            }
            let job = {
                let lock = work_rx.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                lock.recv()
            };
            match job {
                Ok((url, reqs)) => {
                    run_relay_thread(url, reqs, msg_tx.clone(), global_deadline);
                }
                Err(_) => return,
            }
        });
    }
    drop(msg_tx);

    (msg_rx, workers, global_deadline)
}

fn run_relay_thread(
    relay_url: String,
    reqs: Vec<ContentReq>,
    tx: mpsc::Sender<RelayEvent>,
    deadline: Instant,
) {
    let started = Instant::now();

    // Per-sub bookkeeping. One row per (relay, sub_id) so no REQ is hidden
    // behind a relay-only aggregate.
    let send_done = |sub_id: &str, stats: RelayStats| {
        let _ = tx.send(RelayEvent::Done {
            relay: relay_url.clone(),
            sub_id: sub_id.to_string(),
            stats,
        });
    };

    for r in &reqs {
        let _ = tx.send(RelayEvent::Connecting {
            relay: relay_url.clone(),
            sub_id: r.sub_id.clone(),
        });
    }

    let mut socket = match try_connect_msg(&relay_url) {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("error: {e}");
            for r in &reqs {
                let _ = tx.send(RelayEvent::Error {
                    relay: relay_url.clone(),
                    sub_id: r.sub_id.clone(),
                    msg: msg.clone(),
                });
                send_done(
                    &r.sub_id,
                    RelayStats {
                        authors_in_req: r.authors,
                        error: Some(msg.clone()),
                        elapsed: Some(started.elapsed()),
                        ..Default::default()
                    },
                );
            }
            return;
        }
    };

    // Send every content REQ the lifecycle assigned to this relay, verbatim.
    // `stats_for` tracks each sub independently.
    let mut stats_for: std::collections::BTreeMap<String, RelayStats> =
        std::collections::BTreeMap::new();
    let mut open: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for r in &reqs {
        let mut st = RelayStats {
            authors_in_req: r.authors,
            connected: true,
            ..Default::default()
        };
        let raw_filter: Value = match serde_json::from_str(&r.filter_json) {
            Ok(v) => v,
            Err(_) => {
                st.error = Some("bad filter json".to_string());
                st.elapsed = Some(started.elapsed());
                let _ = tx.send(RelayEvent::Error {
                    relay: relay_url.clone(),
                    sub_id: r.sub_id.clone(),
                    msg: "bad filter json".to_string(),
                });
                send_done(&r.sub_id, st);
                continue;
            }
        };
        let req = json!(["REQ", r.sub_id, raw_filter]).to_string();
        if let Err(e) = socket.send(Message::Text(req)) {
            let msg = format!("send REQ: {e}");
            st.error = Some(msg.clone());
            st.elapsed = Some(started.elapsed());
            let _ = tx.send(RelayEvent::Error {
                relay: relay_url.clone(),
                sub_id: r.sub_id.clone(),
                msg,
            });
            send_done(&r.sub_id, st);
            continue;
        }
        let _ = tx.send(RelayEvent::ReqSent {
            relay: relay_url.clone(),
            sub_id: r.sub_id.clone(),
        });
        open.insert(r.sub_id.clone());
        stats_for.insert(r.sub_id.clone(), st);
    }

    // A socket-wide terminal (AUTH / relay close / IO) ends EVERY open sub.
    let mut socket_terminal: Option<String> = None;
    while !open.is_empty() && Instant::now() < deadline {
        match next_frame(&mut socket) {
            Frame::Timeout | Frame::Other => continue,
            Frame::Event { sub_id, event } => {
                if !open.contains(&sub_id) {
                    continue;
                }
                if let Some(st) = stats_for.get_mut(&sub_id) {
                    if st.time_to_first.is_none() {
                        st.time_to_first = Some(started.elapsed());
                    }
                    st.events += 1;
                }
                if let Some(id) = event.get("id").and_then(Value::as_str) {
                    let _ = tx.send(RelayEvent::Frame {
                        relay: relay_url.clone(),
                        sub_id: sub_id.clone(),
                        event_id: id.to_string(),
                    });
                }
            }
            Frame::Eose { sub_id } => {
                if open.remove(&sub_id) {
                    if let Some(st) = stats_for.get_mut(&sub_id) {
                        st.eose = true;
                        st.elapsed = Some(started.elapsed());
                    }
                    let _ = tx.send(RelayEvent::Eose {
                        relay: relay_url.clone(),
                        sub_id: sub_id.clone(),
                        elapsed: started.elapsed(),
                    });
                }
            }
            Frame::Closed { sub_id, message } => {
                if open.remove(&sub_id) {
                    if let Some(st) = stats_for.get_mut(&sub_id) {
                        st.error = Some(message.clone());
                        st.elapsed = Some(started.elapsed());
                    }
                    let _ = tx.send(RelayEvent::Closed {
                        relay: relay_url.clone(),
                        sub_id: sub_id.clone(),
                        msg: message,
                    });
                }
            }
            Frame::Notice { message } => {
                // Non-terminal: surface but keep streaming. Tagged to every
                // open sub on this socket (NOTICE is not sub-scoped).
                for sid in open.iter() {
                    let _ = tx.send(RelayEvent::Notice {
                        relay: relay_url.clone(),
                        sub_id: sid.clone(),
                        msg: message.clone(),
                    });
                }
            }
            Frame::Auth { .. } => {
                socket_terminal = Some("__auth__".to_string());
                break;
            }
            Frame::RelayClosed => {
                socket_terminal = Some("connection closed by relay".to_string());
                break;
            }
            Frame::Io { kind } => {
                socket_terminal = Some(format!("error: {kind}"));
                break;
            }
        }
    }

    // Resolve every still-open sub: socket-wide terminal, or wall timeout.
    let still_open: Vec<String> = open.iter().cloned().collect();
    for sub_id in still_open {
        match &socket_terminal {
            Some(s) if s == "__auth__" => {
                let _ = tx.send(RelayEvent::Auth {
                    relay: relay_url.clone(),
                    sub_id: sub_id.clone(),
                });
                if let Some(st) = stats_for.get_mut(&sub_id) {
                    st.error = Some("AUTH required".to_string());
                }
            }
            Some(reason) => {
                let _ = tx.send(RelayEvent::Error {
                    relay: relay_url.clone(),
                    sub_id: sub_id.clone(),
                    msg: reason.clone(),
                });
                if let Some(st) = stats_for.get_mut(&sub_id) {
                    st.error = Some(reason.clone());
                }
            }
            None => {
                // Wall deadline with no terminal frame — renderer maps a
                // non-terminal row to Timeout itself; just close stats out.
            }
        }
    }

    for r in &reqs {
        let _ = socket.send(Message::Text(json!(["CLOSE", r.sub_id]).to_string()));
    }
    let _ = socket.close(None);
    for r in &reqs {
        let mut st = stats_for.remove(&r.sub_id).unwrap_or(RelayStats {
            authors_in_req: r.authors,
            connected: true,
            ..Default::default()
        });
        if st.elapsed.is_none() {
            st.elapsed = Some(started.elapsed());
        }
        send_done(&r.sub_id, st);
    }
}

// ── tests ────────────────────────────────────────────────────────────────────
//
// These cover the NON-network logic only: empty-input handling, the worker
// pool's URL-scheme filter + worker-count math, the `run_discovery` grouping,
// and the data-struct invariants. Anything that would dial a relay (a valid
// `wss://` job in `launch`, a reachable host in `run_discovery`) is
// deliberately excluded — the REPL's transport is exercised by integration
// runs, not unit tests.

#[cfg(test)]
mod tests {
    use super::*;

    fn req(relay: &str, sub: &str, authors: usize) -> ContentReq {
        ContentReq {
            relay: relay.to_string(),
            sub_id: sub.to_string(),
            filter_json: "{\"kinds\":[1]}".to_string(),
            authors,
        }
    }

    // ── RelayStats invariants ────────────────────────────────────────────

    #[test]
    fn relay_stats_default_is_empty() {
        let s = RelayStats::default();
        assert_eq!(s.events, 0);
        assert_eq!(s.authors_in_req, 0);
        assert!(s.time_to_first.is_none());
        assert!(!s.connected);
        assert!(!s.eose);
        assert!(s.error.is_none());
        assert!(s.elapsed.is_none());
    }

    // ── launch: empty + invalid-scheme inputs (no network) ───────────────

    #[test]
    fn launch_empty_map_clamps_workers_to_one_and_disconnects() {
        // No jobs at all: total_jobs=0 → workers clamped to max(1)=1, and the
        // dropped work channel makes that worker exit immediately. The event
        // receiver must then observe a clean disconnect (no panic, no hang).
        let per_relay: BTreeMap<String, Vec<ContentReq>> = BTreeMap::new();
        let (rx, workers, _deadline) = launch(&per_relay, Duration::from_millis(50));
        assert_eq!(workers, 1, "worker count clamps to a floor of 1");
        // Every sender is dropped once the (only) worker exits → Disconnected.
        // A bounded wait keeps the test from hanging if that ever regresses.
        match rx.recv_timeout(Duration::from_secs(2)) {
            Err(mpsc::RecvTimeoutError::Disconnected) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {
                panic!("worker never exited — channel should disconnect promptly")
            }
            Ok(ev) => panic!("expected no events from an empty plan, got {ev:?}"),
        }
    }

    #[test]
    fn launch_filters_out_non_ws_scheme_urls() {
        // Only non-ws URLs: every job is filtered → total_jobs=0 → workers=1,
        // and no thread ever dials anything. Confirms the scheme guard in
        // `launch` keeps `http`/`ftp`/bare-host entries off the wire.
        let mut per_relay: BTreeMap<String, Vec<ContentReq>> = BTreeMap::new();
        per_relay.insert(
            "http://relay.example".to_string(),
            vec![req("http://relay.example", "s1", 1)],
        );
        per_relay.insert(
            "ftp://relay.example".to_string(),
            vec![req("ftp://relay.example", "s2", 1)],
        );
        per_relay.insert(
            "relay.example".to_string(),
            vec![req("relay.example", "s3", 1)],
        );
        let (rx, workers, _deadline) = launch(&per_relay, Duration::from_millis(50));
        assert_eq!(workers, 1, "all non-ws jobs filtered → worker floor of 1");
        // No job queued → the lone worker exits immediately, channel closes.
        assert!(
            matches!(
                rx.recv_timeout(Duration::from_secs(2)),
                Err(mpsc::RecvTimeoutError::Disconnected)
            ),
            "no relay events when every URL is filtered"
        );
    }

    #[test]
    fn launch_returns_future_deadline() {
        let per_relay: BTreeMap<String, Vec<ContentReq>> = BTreeMap::new();
        let wall = Duration::from_secs(10);
        let before = Instant::now();
        let (_rx, _workers, deadline) = launch(&per_relay, wall);
        // The deadline is `now + wall`; it must lie in the future window.
        assert!(deadline > before);
        assert!(deadline <= Instant::now() + wall);
    }

    // ── run_discovery: empty input (no network) ──────────────────────────

    #[test]
    fn run_discovery_empty_probes_returns_empty() {
        // No probes → empty `by_relay` → the relay loop never runs → no
        // socket is ever opened. Pure, deterministic, network-free.
        let out = run_discovery(&[]);
        assert!(out.is_empty());
    }

    // ── ContentReq / RelayEvent are constructible + cloneable ────────────

    #[test]
    fn content_req_clone_preserves_fields() {
        let r = req("wss://relay.example", "sub-1", 42);
        let c = r.clone();
        assert_eq!(c.relay, "wss://relay.example");
        assert_eq!(c.sub_id, "sub-1");
        assert_eq!(c.authors, 42);
        assert_eq!(c.filter_json, "{\"kinds\":[1]}");
    }
}
