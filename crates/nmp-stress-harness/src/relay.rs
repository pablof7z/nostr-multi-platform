//! Embedded in-process fixture relay.
//!
//! A minimal NIP-01 `ws://` relay: it accepts connections, stores every
//! `["EVENT", <event>]` a client publishes, replies `["OK", id, true, ""]`,
//! and on `["REQ", subid, <filter>...]` replays every **stored** event that
//! matches the filter as `["EVENT", subid, <event>]` followed by
//! `["EOSE", subid]`. It also keeps the subscription open so that events
//! published (by anyone — including a *second* relay-staged event we push
//! through `stage_event`) AFTER the REQ are pushed live to matching open subs.
//!
//! The catalog notes no in-process mock relay exists in `nmp-testing`, so this
//! is the minimal one the relay-echo / dedup / sibling-relay / out-of-order /
//! foreign-author-injection scenarios drive. The NMP native relay worker
//! (`tungstenite` + `mio`) connects to it over a real WebSocket; inbound text
//! frames flow through the real `Kernel::handle_event` → `verify_and_persist`
//! chokepoint, so this is genuinely the landed ingest path — not a kernel
//! bypass.
//!
//! Design constraints honoured:
//! - One acceptor thread + one thread per connection (blocking I/O; no async
//!   runtime, matching the worker's blocking-socket posture).
//! - `stage_event` lets a scenario pre-load an event the relay will deliver to
//!   any client that REQs a matching filter — this is how we inject
//!   foreign-author / future-dated / kind:5-delete / replaceable-sibling events
//!   through the REAL relay path.

// `has_event` is part of the reusable fixture-relay surface used by
// not-yet-landed scenarios; allow until they consume it.
#![allow(dead_code)]

use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::Value;

/// Shared relay state: the set of stored events plus the live subscriptions
/// (so a later EVENT — or a `stage_event` — fans out to open REQs).
#[derive(Default)]
struct RelayState {
    /// id -> raw event JSON value (NIP-01 object). Insertion order preserved
    /// via `order` for deterministic replay.
    events: HashMap<String, Value>,
    order: Vec<String>,
    /// Per-connection live subscriptions: conn_id -> (subid -> filters).
    subs: HashMap<u64, HashMap<String, Vec<Value>>>,
    /// Per-connection outbound senders so a published / staged event can be
    /// pushed live to matching open subs on other connections.
    outbound: HashMap<u64, Sender<String>>,
    next_conn: u64,
    /// Count of distinct event ids the relay has *received from clients*
    /// (publish path). Lets a scenario assert "the publish reached the relay".
    received_publishes: u64,
}

/// Handle to a running fixture relay. `url()` is the `ws://127.0.0.1:PORT`
/// the NMP app should be pointed at. Dropping the handle leaves the acceptor
/// thread running (process exit reaps it) — harness scenarios are short-lived.
#[derive(Clone)]
pub struct FixtureRelay {
    url: String,
    state: Arc<Mutex<RelayState>>,
}

impl FixtureRelay {
    /// Bind an ephemeral localhost port and start accepting connections.
    pub fn start() -> FixtureRelay {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture relay");
        let port = listener.local_addr().expect("local_addr").port();
        let url = format!("ws://127.0.0.1:{port}");
        let state = Arc::new(Mutex::new(RelayState::default()));
        let acceptor_state = Arc::clone(&state);
        thread::Builder::new()
            .name("fixture-relay-acceptor".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { continue };
                    let conn_state = Arc::clone(&acceptor_state);
                    thread::Builder::new()
                        .name("fixture-relay-conn".into())
                        .spawn(move || serve_connection(stream, conn_state))
                        .ok();
                }
            })
            .expect("spawn acceptor");
        FixtureRelay { url, state }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// Pre-load / inject an event into the relay's store and fan it out to any
    /// currently-open matching subscription. This is the injection vector for
    /// foreign-author / future-dated / delete / replaceable-sibling events: a
    /// scenario stages the event, then the NMP app's REQ (or an already-open
    /// REQ) pulls it through the real ingest chokepoint.
    pub fn stage_event(&self, event_json: &Value) {
        let id = event_json
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if id.is_empty() {
            return;
        }
        let mut st = self.state.lock().expect("relay lock");
        if !st.events.contains_key(&id) {
            st.order.push(id.clone());
        }
        st.events.insert(id.clone(), event_json.clone());
        // Fan out live to matching open subs.
        fanout_live(&st, &id, event_json);
    }

    /// Number of distinct event ids this relay has received from clients via a
    /// `["EVENT", ...]` publish frame. Used to assert a publish actually hit
    /// the wire (e.g. RYW echo / dedup scenarios).
    pub fn received_publishes(&self) -> u64 {
        self.state.lock().map(|s| s.received_publishes).unwrap_or(0)
    }

    /// Whether the relay currently has this event id in its store (from a
    /// publish or a `stage_event`).
    pub fn has_event(&self, id: &str) -> bool {
        self.state
            .lock()
            .map(|s| s.events.contains_key(id))
            .unwrap_or(false)
    }
}

/// Fan a (just-stored) event out to every open subscription whose filter set
/// matches it. Caller holds the lock.
fn fanout_live(st: &RelayState, _id: &str, event: &Value) {
    for (conn_id, subs) in &st.subs {
        let Some(tx) = st.outbound.get(conn_id) else {
            continue;
        };
        for (subid, filters) in subs {
            if filters_match(filters, event) {
                let frame = serde_json::json!(["EVENT", subid, event]).to_string();
                let _ = tx.send(frame);
            }
        }
    }
}

fn serve_connection(stream: TcpStream, state: Arc<Mutex<RelayState>>) {
    // Blocking WebSocket handshake (server side).
    let Ok(mut ws) = tungstenite::accept(stream) else {
        return;
    };
    // Assign a connection id and register an outbound channel. A dedicated
    // writer is not used; instead the read loop drains pending outbound frames
    // after each read with a short read timeout. To keep this dependency-free
    // and simple we use a non-blocking approach: set a read timeout on the
    // underlying stream so the loop wakes to flush live frames.
    let conn_id = {
        let mut st = state.lock().expect("lock");
        let id = st.next_conn;
        st.next_conn += 1;
        id
    };
    let (tx, rx) = channel::<String>();
    {
        let mut st = state.lock().expect("lock");
        st.outbound.insert(conn_id, tx);
        st.subs.insert(conn_id, HashMap::new());
    }

    // Set a short read timeout so the loop can interleave reads with draining
    // live outbound frames (pushed by other connections' publishes / staged
    // events). tungstenite exposes the underlying stream via `get_ref`.
    let _ = ws
        .get_ref()
        .set_read_timeout(Some(std::time::Duration::from_millis(25)));

    loop {
        // Drain any pending live frames first.
        while let Ok(frame) = rx.try_recv() {
            if ws.send(tungstenite::Message::Text(frame)).is_err() {
                cleanup(&state, conn_id);
                return;
            }
        }
        match ws.read() {
            Ok(tungstenite::Message::Text(text)) => {
                handle_client_frame(&mut ws, &state, conn_id, &text);
            }
            Ok(tungstenite::Message::Ping(p)) => {
                let _ = ws.send(tungstenite::Message::Pong(p));
            }
            Ok(tungstenite::Message::Close(_)) => {
                cleanup(&state, conn_id);
                return;
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                // Read timeout — loop back to flush outbound frames.
                continue;
            }
            Err(_) => {
                cleanup(&state, conn_id);
                return;
            }
        }
    }
}

fn cleanup(state: &Arc<Mutex<RelayState>>, conn_id: u64) {
    if let Ok(mut st) = state.lock() {
        st.subs.remove(&conn_id);
        st.outbound.remove(&conn_id);
    }
}

fn handle_client_frame(
    ws: &mut tungstenite::WebSocket<TcpStream>,
    state: &Arc<Mutex<RelayState>>,
    conn_id: u64,
    text: &str,
) {
    let Ok(arr) = serde_json::from_str::<Vec<Value>>(text) else {
        return;
    };
    let Some(verb) = arr.first().and_then(Value::as_str) else {
        return;
    };
    match verb {
        "EVENT" => {
            // ["EVENT", <event>]
            if let Some(event) = arr.get(1) {
                let id = event
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if !id.is_empty() {
                    let mut st = state.lock().expect("lock");
                    let is_new = !st.events.contains_key(&id);
                    if is_new {
                        st.order.push(id.clone());
                        st.received_publishes += 1;
                    }
                    st.events.insert(id.clone(), event.clone());
                    fanout_live(&st, &id, event);
                    drop(st);
                    let ok = serde_json::json!(["OK", id, true, ""]).to_string();
                    let _ = ws.send(tungstenite::Message::Text(ok));
                }
            }
        }
        "REQ" => {
            // ["REQ", subid, filter, filter, ...]
            let Some(subid) = arr.get(1).and_then(Value::as_str) else {
                return;
            };
            let filters: Vec<Value> = arr.iter().skip(2).cloned().collect();
            // Replay matching stored events in insertion order, then EOSE.
            let mut to_send: Vec<String> = Vec::new();
            {
                let mut st = state.lock().expect("lock");
                for id in &st.order {
                    if let Some(ev) = st.events.get(id) {
                        if filters_match(&filters, ev) {
                            to_send.push(serde_json::json!(["EVENT", subid, ev]).to_string());
                        }
                    }
                }
                // Register the live sub so post-REQ events fan out.
                st.subs
                    .entry(conn_id)
                    .or_default()
                    .insert(subid.to_string(), filters);
            }
            for frame in to_send {
                if ws.send(tungstenite::Message::Text(frame)).is_err() {
                    return;
                }
            }
            let eose = serde_json::json!(["EOSE", subid]).to_string();
            let _ = ws.send(tungstenite::Message::Text(eose));
        }
        "CLOSE" => {
            if let Some(subid) = arr.get(1).and_then(Value::as_str) {
                let mut st = state.lock().expect("lock");
                if let Some(subs) = st.subs.get_mut(&conn_id) {
                    subs.remove(subid);
                }
            }
        }
        _ => {}
    }
}

/// NIP-01 filter matching, minimal but correct for the dimensions the harness
/// exercises: `ids`, `authors`, `kinds`, `#e`/`#p` tag filters, `since`,
/// `until`. An empty filter set (no filters) matches everything; an event
/// matches if it matches ANY filter in the set (OR across filters, AND within
/// a filter).
fn filters_match(filters: &[Value], event: &Value) -> bool {
    if filters.is_empty() {
        return true;
    }
    filters.iter().any(|f| single_filter_matches(f, event))
}

fn single_filter_matches(filter: &Value, event: &Value) -> bool {
    let Some(obj) = filter.as_object() else {
        return false;
    };
    for (key, want) in obj {
        match key.as_str() {
            "ids" => {
                let id = event.get("id").and_then(Value::as_str).unwrap_or_default();
                if !value_array_contains_str(want, id) {
                    return false;
                }
            }
            "authors" => {
                let pk = event.get("pubkey").and_then(Value::as_str).unwrap_or_default();
                if !value_array_contains_str(want, pk) {
                    return false;
                }
            }
            "kinds" => {
                let kind = event.get("kind").and_then(Value::as_u64).unwrap_or(u64::MAX);
                let ok = want
                    .as_array()
                    .map(|a| a.iter().any(|k| k.as_u64() == Some(kind)))
                    .unwrap_or(false);
                if !ok {
                    return false;
                }
            }
            "since" => {
                let since = want.as_u64().unwrap_or(0);
                let created = event.get("created_at").and_then(Value::as_u64).unwrap_or(0);
                if created < since {
                    return false;
                }
            }
            "until" => {
                let until = want.as_u64().unwrap_or(u64::MAX);
                let created = event.get("created_at").and_then(Value::as_u64).unwrap_or(0);
                if created > until {
                    return false;
                }
            }
            k if k.starts_with('#') && k.len() == 2 => {
                let tag_name = &k[1..];
                if !event_has_tag_in(event, tag_name, want) {
                    return false;
                }
            }
            // limit / other fields: not a match constraint here.
            _ => {}
        }
    }
    true
}

fn value_array_contains_str(arr: &Value, needle: &str) -> bool {
    arr.as_array()
        .map(|a| a.iter().any(|v| v.as_str() == Some(needle)))
        .unwrap_or(false)
}

fn event_has_tag_in(event: &Value, tag_name: &str, want: &Value) -> bool {
    let Some(tags) = event.get("tags").and_then(Value::as_array) else {
        return false;
    };
    let wanted: Vec<&str> = want
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if wanted.is_empty() {
        return false;
    }
    tags.iter().any(|t| {
        let Some(t) = t.as_array() else { return false };
        let name = t.first().and_then(Value::as_str).unwrap_or_default();
        let val = t.get(1).and_then(Value::as_str).unwrap_or_default();
        name == tag_name && wanted.contains(&val)
    })
}
