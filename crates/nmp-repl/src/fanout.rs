//! Bounded worker pool for the per-relay REQ fan-out. Verbatim port of
//! `outbox_perf.rs::phase_d_fanout` + `run_relay_thread`, with `Msg` replaced
//! by `RelayEvent` so the renderer can paint progress.
//!
//! Pitfall §13.5 — workers are detached at the wall deadline; no graceful
//! cancellation. Sockets drop on scope exit; OS reclaims them.

use std::collections::BTreeMap;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::Message;

use crate::ast::FilterAst;
use crate::ws::{next_text, try_connect};

const FANOUT_MAX_WORKERS: usize = 64;

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
    Connecting {
        relay: String,
    },
    ReqSent {
        relay: String,
    },
    Frame {
        relay: String,
        event_id: String,
    },
    Eose {
        relay: String,
        elapsed: Duration,
    },
    Error {
        relay: String,
        msg: String,
    },
    Done {
        relay: String,
        stats: RelayStats,
    },
}

/// Build the per-relay REQ filter from the parsed AST. Used by workers to
/// shape the wire frame.
fn build_filter_json(filter: &FilterAst, authors: &[String]) -> Value {
    let mut obj = serde_json::Map::new();
    if let Some(kinds) = &filter.kinds {
        obj.insert("kinds".to_string(), json!(kinds));
    }
    if !authors.is_empty() {
        obj.insert("authors".to_string(), json!(authors));
    }
    if let Some(ids) = &filter.ids {
        let lits: Vec<String> = ids
            .iter()
            .filter_map(|v| {
                if let crate::ast::Value::Lit(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .collect();
        if !lits.is_empty() {
            obj.insert("ids".to_string(), json!(lits));
        }
    }
    if let Some(since) = filter.since {
        obj.insert("since".to_string(), json!(since));
    }
    if let Some(until) = filter.until {
        obj.insert("until".to_string(), json!(until));
    }
    if let Some(limit) = filter.limit {
        obj.insert("limit".to_string(), json!(limit));
    }
    for (letter, values) in &filter.tags {
        let lits: Vec<String> = values
            .iter()
            .filter_map(|v| {
                if let crate::ast::Value::Lit(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .collect();
        if !lits.is_empty() {
            obj.insert(format!("#{letter}"), json!(lits));
        }
    }
    Value::Object(obj)
}

/// Launch the worker pool. Returns the receiver and the worker count.
/// The receiver yields `RelayEvent`s; when the channel closes (all workers
/// dropped their `Sender`) or the wall deadline elapses, fan-out is done.
pub fn launch(
    per_relay: &BTreeMap<String, Vec<String>>,
    filter: FilterAst,
    wall: Duration,
) -> (mpsc::Receiver<RelayEvent>, usize, Instant) {
    let (msg_tx, msg_rx) = mpsc::channel::<RelayEvent>();
    let (work_tx, work_rx) = mpsc::channel::<(String, Vec<String>)>();
    let work_rx = Arc::new(Mutex::new(work_rx));
    let global_deadline = Instant::now() + wall;

    let mut total_jobs = 0usize;
    for (relay_url, authors) in per_relay {
        if !relay_url.starts_with("wss://") && !relay_url.starts_with("ws://") {
            continue;
        }
        work_tx
            .send((relay_url.clone(), authors.clone()))
            .expect("queue job");
        total_jobs += 1;
    }
    drop(work_tx);

    let workers = FANOUT_MAX_WORKERS.min(total_jobs.max(1));
    for _ in 0..workers {
        let work_rx = work_rx.clone();
        let msg_tx = msg_tx.clone();
        let filter = filter.clone();
        thread::spawn(move || loop {
            if Instant::now() >= global_deadline {
                return;
            }
            let job = {
                let lock = work_rx.lock().unwrap();
                lock.try_recv()
            };
            match job {
                Ok((url, authors)) => {
                    run_relay_thread(url, authors, &filter, msg_tx.clone(), global_deadline);
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
    authors: Vec<String>,
    filter: &FilterAst,
    tx: mpsc::Sender<RelayEvent>,
    deadline: Instant,
) {
    let authors_in_req = authors.len();
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

    let sub_id = "feed-1";
    let filter_json = build_filter_json(filter, &authors);
    let req = json!(["REQ", sub_id, filter_json]).to_string();
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
    let _ = tx.send(RelayEvent::ReqSent {
        relay: relay_url.clone(),
    });

    while Instant::now() < deadline {
        match next_text(&mut socket) {
            None => continue,
            Some(text) => {
                if text.is_empty() {
                    // benign close/error frame — stop reading.
                    break;
                }
                let v: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                match v[0].as_str() {
                    Some("EVENT") if v[1].as_str() == Some(sub_id) => {
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
                    Some("EOSE") if v[1].as_str() == Some(sub_id) => {
                        stats.eose = true;
                        let elapsed = started.elapsed();
                        let _ = tx.send(RelayEvent::Eose {
                            relay: relay_url.clone(),
                            elapsed,
                        });
                        break;
                    }
                    Some("NOTICE") => {
                        // Surface relay-side notices as a transient error
                        // (not fatal — keep reading).
                        if let Some(msg) = v.get(1).and_then(Value::as_str) {
                            let _ = tx.send(RelayEvent::Error {
                                relay: relay_url.clone(),
                                msg: format!("NOTICE: {msg}"),
                            });
                        }
                    }
                    Some("CLOSED") if v[1].as_str() == Some(sub_id) => {
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
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    let _ = socket.send(Message::Text(json!(["CLOSE", sub_id]).to_string()));
    let _ = socket.close(None);
    stats.elapsed = Some(started.elapsed());
    let _ = tx.send(RelayEvent::Done {
        relay: relay_url,
        stats,
    });
}
