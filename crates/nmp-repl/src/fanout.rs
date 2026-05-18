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
use crate::ws::{next_text, try_connect};
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

/// Events emitted from a worker to the main thread.
#[derive(Debug)]
pub enum RelayEvent {
    Connecting { relay: String },
    ReqSent { relay: String },
    Frame { relay: String, event_id: String },
    Eose { relay: String, elapsed: Duration },
    Error { relay: String, msg: String },
    Done { relay: String, stats: RelayStats },
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

    let mut out: BTreeMap<String, MailboxSnapshot> = BTreeMap::new();
    for (relay, subs) in by_relay {
        let mut socket = match try_connect(&relay) {
            Some(s) => s,
            None => continue,
        };
        let mut open: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for (sub_id, filter) in &subs {
            let raw_filter: Value =
                serde_json::from_str(filter).unwrap_or_else(|_| json!({ "kinds": [10002] }));
            let req = json!(["REQ", sub_id, raw_filter]).to_string();
            if socket.send(Message::Text(req)).is_ok() {
                open.insert(sub_id.clone());
            }
        }
        let deadline = Instant::now() + DISCOVERY_WALL;
        while !open.is_empty() && Instant::now() < deadline {
            match next_text(&mut socket) {
                None => continue,
                Some(text) => {
                    if text.is_empty() {
                        break;
                    }
                    let v: Value = match serde_json::from_str(&text) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    match v[0].as_str() {
                        Some("EVENT") => {
                            if let Some(event) = v.get(2) {
                                if let Some((pk, snap)) = parse_kind10002(event) {
                                    out.insert(pk, snap);
                                }
                            }
                        }
                        Some("EOSE") => {
                            if let Some(sid) = v[1].as_str() {
                                open.remove(sid);
                            }
                        }
                        _ => {}
                    }
                }
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
        work_tx
            .send((relay_url.clone(), reqs.clone()))
            .expect("queue job");
        total_jobs += 1;
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
                let lock = work_rx.lock().unwrap();
                lock.try_recv()
            };
            match job {
                Ok((url, reqs)) => {
                    run_relay_thread(url, reqs, msg_tx.clone(), global_deadline);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(mpsc::TryRecvError::Disconnected) => return,
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
    let authors_in_req = reqs.iter().map(|r| r.authors).sum();
    let mut stats = RelayStats {
        events: 0,
        authors_in_req,
        time_to_first: None,
        connected: false,
        eose: false,
        error: None,
        elapsed: None,
    };
    let started = Instant::now();

    let _ = tx.send(RelayEvent::Connecting {
        relay: relay_url.clone(),
    });

    let mut socket = match try_connect(&relay_url) {
        Some(s) => s,
        None => {
            stats.error = Some("connect refused".to_string());
            let _ = tx.send(RelayEvent::Error {
                relay: relay_url.clone(),
                msg: "connect refused".to_string(),
            });
            stats.elapsed = Some(started.elapsed());
            let _ = tx.send(RelayEvent::Done {
                relay: relay_url,
                stats,
            });
            return;
        }
    };
    stats.connected = true;

    // Send every content REQ the lifecycle assigned to this relay, verbatim.
    let mut open: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for r in &reqs {
        let raw_filter: Value = match serde_json::from_str(&r.filter_json) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let req = json!(["REQ", r.sub_id, raw_filter]).to_string();
        if let Err(e) = socket.send(Message::Text(req)) {
            let msg = format!("send REQ: {e}");
            stats.error = Some(msg.clone());
            let _ = tx.send(RelayEvent::Error {
                relay: relay_url.clone(),
                msg,
            });
            stats.elapsed = Some(started.elapsed());
            let _ = tx.send(RelayEvent::Done {
                relay: relay_url,
                stats,
            });
            return;
        }
        open.insert(r.sub_id.clone());
    }
    let _ = tx.send(RelayEvent::ReqSent {
        relay: relay_url.clone(),
    });

    while !open.is_empty() && Instant::now() < deadline {
        match next_text(&mut socket) {
            None => continue,
            Some(text) => {
                if text.is_empty() {
                    break;
                }
                let v: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let sid = v.get(1).and_then(Value::as_str).unwrap_or("");
                match v[0].as_str() {
                    Some("EVENT") if open.contains(sid) => {
                        if stats.time_to_first.is_none() {
                            stats.time_to_first = Some(started.elapsed());
                        }
                        stats.events += 1;
                        if let Some(id) = v
                            .get(2)
                            .and_then(|v| v.get("id"))
                            .and_then(Value::as_str)
                        {
                            let _ = tx.send(RelayEvent::Frame {
                                relay: relay_url.clone(),
                                event_id: id.to_string(),
                            });
                        }
                    }
                    Some("EOSE") if open.contains(sid) => {
                        open.remove(sid);
                        if open.is_empty() {
                            stats.eose = true;
                            let elapsed = started.elapsed();
                            let _ = tx.send(RelayEvent::Eose {
                                relay: relay_url.clone(),
                                elapsed,
                            });
                            break;
                        }
                    }
                    Some("NOTICE") => {
                        if let Some(msg) = v.get(1).and_then(Value::as_str) {
                            let _ = tx.send(RelayEvent::Error {
                                relay: relay_url.clone(),
                                msg: format!("NOTICE: {msg}"),
                            });
                        }
                    }
                    Some("CLOSED") if open.contains(sid) => {
                        let msg = v
                            .get(2)
                            .and_then(Value::as_str)
                            .unwrap_or("CLOSED")
                            .to_string();
                        stats.error = Some(msg.clone());
                        let _ = tx.send(RelayEvent::Error {
                            relay: relay_url.clone(),
                            msg,
                        });
                        open.remove(sid);
                        if open.is_empty() {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    for r in &reqs {
        let _ = socket.send(Message::Text(json!(["CLOSE", r.sub_id]).to_string()));
    }
    let _ = socket.close(None);
    stats.elapsed = Some(started.elapsed());
    let _ = tx.send(RelayEvent::Done {
        relay: relay_url,
        stats,
    });
}
