use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc;
use std::time::Instant;

use serde_json::{json, Value};
use tungstenite::Message;

use crate::ws::{next_frame, try_connect_msg, Frame};
use super::{ContentReq, RelayEvent, RelayStats};

/// Socket-wide terminal condition for a relay worker.
pub(super) enum SocketTerminal {
    AuthRequired,
    Error(String),
}

pub(super) fn run_relay_thread(
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
    let mut stats_for: BTreeMap<String, RelayStats> = BTreeMap::new();
    let mut open: BTreeSet<String> = BTreeSet::new();
    for r in &reqs {
        let mut st = RelayStats {
            authors_in_req: r.authors,
            connected: true,
            ..Default::default()
        };
        let raw_filter: Value = if let Ok(v) = serde_json::from_str(&r.filter_json) { v } else {
            st.error = Some("bad filter json".to_string());
            st.elapsed = Some(started.elapsed());
            let _ = tx.send(RelayEvent::Error {
                relay: relay_url.clone(),
                sub_id: r.sub_id.clone(),
                msg: "bad filter json".to_string(),
            });
            send_done(&r.sub_id, st);
            continue;
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
    let mut socket_terminal: Option<SocketTerminal> = None;
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
                for sid in &open {
                    let _ = tx.send(RelayEvent::Notice {
                        relay: relay_url.clone(),
                        sub_id: sid.clone(),
                        msg: message.clone(),
                    });
                }
            }
            Frame::Auth { .. } => {
                socket_terminal = Some(SocketTerminal::AuthRequired);
                break;
            }
            Frame::RelayClosed => {
                socket_terminal = Some(SocketTerminal::Error(
                    "connection closed by relay".to_string(),
                ));
                break;
            }
            Frame::Io { kind } => {
                socket_terminal = Some(SocketTerminal::Error(format!("error: {kind}")));
                break;
            }
        }
    }

    // Resolve every still-open sub: socket-wide terminal, or wall timeout.
    let still_open: Vec<String> = open.iter().cloned().collect();
    for sub_id in still_open {
        match &socket_terminal {
            Some(SocketTerminal::AuthRequired) => {
                let _ = tx.send(RelayEvent::Auth {
                    relay: relay_url.clone(),
                    sub_id: sub_id.clone(),
                });
                if let Some(st) = stats_for.get_mut(&sub_id) {
                    st.error = Some("AUTH required".to_string());
                }
            }
            Some(SocketTerminal::Error(reason)) => {
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
