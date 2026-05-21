//! Phase D: parallel fan-out to the optimized relay set.

use std::collections::{BTreeMap, HashSet};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::Message;

use super::transport::{next_text, try_connect};

/// Maximum parallel relay workers for the fan-out phase.
pub const FANOUT_MAX_WORKERS: usize = 64;

/// Wall-clock budget for the entire fan-out phase.
const FANOUT_WALL: Duration = Duration::from_secs(20);

/// Per-relay statistics collected during the fan-out.
#[derive(Default, Clone)]
pub struct RelayStats {
    pub events: u64,
    pub authors_in_req: usize,
    pub time_to_first: Option<Duration>,
    pub connected: bool,
    pub eose: bool,
}

enum Msg {
    Frame { relay: String, value: Value },
    Done { relay: String, stats: RelayStats },
}

/// Spin up parallel workers and fan-out REQs to every relay in `per_relay`.
///
/// Returns `(total_deliveries, unique_event_ids, per_relay_stats)`.
pub fn phase_d_fanout(
    per_relay: &BTreeMap<String, Vec<String>>,
) -> (u64, u64, BTreeMap<String, RelayStats>) {
    let (msg_tx, msg_rx) = mpsc::channel::<Msg>();
    let (work_tx, work_rx) = mpsc::channel::<(String, Vec<String>)>();
    let work_rx = Arc::new(Mutex::new(work_rx));
    let global_deadline = Instant::now() + FANOUT_WALL;

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
    println!(
        "phase D — fanout: {} jobs across {} parallel workers (wall {:?})",
        total_jobs, workers, FANOUT_WALL
    );
    for _ in 0..workers {
        let work_rx = work_rx.clone();
        let msg_tx = msg_tx.clone();
        thread::spawn(move || loop {
            let job = {
                let lock = work_rx.lock().unwrap();
                lock.recv_timeout(Duration::from_millis(50))
            };
            match job {
                Ok((url, authors)) => {
                    run_relay_thread(url, authors, msg_tx.clone(), global_deadline);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if Instant::now() >= global_deadline {
                        return;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        });
    }
    drop(msg_tx);

    let mut unique: HashSet<String> = HashSet::new();
    let mut totals = 0u64;
    let mut per_relay_stats: BTreeMap<String, RelayStats> = BTreeMap::new();

    loop {
        let now = Instant::now();
        if now >= global_deadline {
            break;
        }
        let remaining = global_deadline.saturating_duration_since(now);
        let timeout = remaining.min(Duration::from_millis(500));
        let msg = match msg_rx.recv_timeout(timeout) {
            Ok(m) => m,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        match msg {
            Msg::Frame { relay, value } => {
                let stats = per_relay_stats.entry(relay).or_default();
                if let Some(id) = value
                    .get(2)
                    .and_then(|v| v.get("id"))
                    .and_then(Value::as_str)
                {
                    stats.events += 1;
                    totals += 1;
                    unique.insert(id.to_string());
                }
            }
            Msg::Done { relay, stats } => {
                let entry = per_relay_stats.entry(relay).or_default();
                entry.authors_in_req = stats.authors_in_req;
                entry.connected |= stats.connected;
                entry.eose |= stats.eose;
                if entry.time_to_first.is_none() {
                    entry.time_to_first = stats.time_to_first;
                }
            }
        }
    }
    (totals, unique.len() as u64, per_relay_stats)
}

fn run_relay_thread(
    relay_url: String,
    authors: Vec<String>,
    tx: mpsc::Sender<Msg>,
    deadline: Instant,
) {
    let authors_in_req = authors.len();
    let mut stats = RelayStats {
        events: 0,
        authors_in_req,
        time_to_first: None,
        connected: false,
        eose: false,
    };
    let started = Instant::now();

    let mut socket = match try_connect(&relay_url) {
        Some(s) => s,
        None => {
            let _ = tx.send(Msg::Done {
                relay: relay_url,
                stats,
            });
            return;
        }
    };
    stats.connected = true;

    let sub_id = "feed-1";
    let filter = json!({
        "kinds": [1, 6],
        "authors": authors,
        "limit": 200,
    });
    let req = json!(["REQ", sub_id, filter]).to_string();
    if socket.send(Message::Text(req)).is_err() {
        let _ = tx.send(Msg::Done {
            relay: relay_url,
            stats,
        });
        return;
    }

    while Instant::now() < deadline {
        match next_text(&mut socket) {
            None => continue,
            Some(text) => {
                let v: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                match v[0].as_str() {
                    Some("EVENT") if v[1].as_str() == Some(sub_id) => {
                        if stats.time_to_first.is_none() {
                            stats.time_to_first = Some(started.elapsed());
                        }
                        let _ = tx.send(Msg::Frame {
                            relay: relay_url.clone(),
                            value: v,
                        });
                    }
                    Some("EOSE") if v[1].as_str() == Some(sub_id) => {
                        stats.eose = true;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    let _ = socket.send(Message::Text(json!(["CLOSE", sub_id]).to_string()));
    let _ = socket.close(None);
    let _ = tx.send(Msg::Done {
        relay: relay_url,
        stats,
    });
}
