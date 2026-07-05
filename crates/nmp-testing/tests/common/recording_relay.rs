use std::collections::{BTreeMap, BTreeSet};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use nostr::Event;
use serde_json::{json, Value};
use tungstenite::Message;

#[derive(Debug, Clone)]
pub(crate) struct ObservedReq {
    pub sub_id: String,
    pub filter: Value,
}

#[derive(Debug, Clone)]
pub(crate) enum ObservedFrame {
    Req(ObservedReq),
    Close { sub_id: String },
    /// A client-published `["EVENT", …]` frame (a publish landing on the relay).
    Event(Box<Event>),
}

enum RelayCommand {
    Push(Event),
    Stop,
}

pub(crate) struct RecordingRelay {
    addr: SocketAddr,
    url: String,
    stop: Arc<AtomicBool>,
    commands: Sender<RelayCommand>,
    observed_rx: Receiver<ObservedFrame>,
    observed: Vec<ObservedFrame>,
    handle: Option<thread::JoinHandle<()>>,
}

impl RecordingRelay {
    pub(crate) fn spawn(events: Vec<Event>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture relay");
        let addr = listener.local_addr().expect("fixture relay addr");
        let stop = Arc::new(AtomicBool::new(false));
        let stop_t = Arc::clone(&stop);
        let (command_tx, command_rx) = channel();
        let (observed_tx, observed_rx) = channel();
        let handle = thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            if stop_t.load(Ordering::Relaxed) {
                return;
            }
            serve_connection(stream, events, command_rx, observed_tx, stop_t);
        });
        Self {
            addr,
            url: format!("ws://{addr}"),
            stop,
            commands: command_tx,
            observed_rx,
            observed: Vec::new(),
            handle: Some(handle),
        }
    }

    pub(crate) fn url(&self) -> &str {
        &self.url
    }

    pub(crate) fn push_event(&self, event: Event) {
        self.commands
            .send(RelayCommand::Push(event))
            .expect("relay command channel open");
    }

    pub(crate) fn wait_req(&mut self, label: &str, pred: impl Fn(&Value) -> bool) -> ObservedReq {
        match self.wait_frame(
            label,
            |frame| matches!(frame, ObservedFrame::Req(req) if pred(&req.filter)),
        ) {
            ObservedFrame::Req(req) => req,
            _ => unreachable!(),
        }
    }

    /// Wait for a client-published `EVENT` frame matching `pred` (e.g. a
    /// kind:1059 gift-wrap Welcome). Returns the published event.
    pub(crate) fn wait_event(&mut self, label: &str, pred: impl Fn(&Event) -> bool) -> Event {
        match self.wait_frame(
            label,
            |frame| matches!(frame, ObservedFrame::Event(ev) if pred(ev)),
        ) {
            ObservedFrame::Event(ev) => *ev,
            _ => unreachable!(),
        }
    }

    /// Every client-published EVENT observed so far (non-blocking drain).
    pub(crate) fn drain_published(&mut self) -> Vec<Event> {
        while let Ok(frame) = self.observed_rx.try_recv() {
            self.observed.push(frame);
        }
        self.observed
            .iter()
            .filter_map(|f| match f {
                ObservedFrame::Event(ev) => Some((**ev).clone()),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn wait_close(&mut self, label: &str, sub_id: &str) {
        let _ = self.wait_frame(
            label,
            |frame| matches!(frame, ObservedFrame::Close { sub_id: got } if got == sub_id),
        );
    }

    fn wait_frame(&mut self, label: &str, pred: impl Fn(&ObservedFrame) -> bool) -> ObservedFrame {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(pos) = self.observed.iter().position(&pred) {
                return self.observed.remove(pos);
            }
            let now = Instant::now();
            assert!(
                now < deadline,
                "timed out waiting for {label}; observed backlog = {:?}",
                self.observed
            );
            let remaining = deadline.saturating_duration_since(now);
            match self
                .observed_rx
                .recv_timeout(remaining.min(Duration::from_millis(500)))
            {
                Ok(frame) => self.observed.push(frame),
                Err(_) => panic!(
                    "timed out waiting for {label}; observed backlog = {:?}",
                    self.observed
                ),
            }
        }
    }
}

impl Drop for RecordingRelay {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.commands.send(RelayCommand::Stop);
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn serve_connection(
    stream: TcpStream,
    mut events: Vec<Event>,
    commands: Receiver<RelayCommand>,
    observed: Sender<ObservedFrame>,
    stop: Arc<AtomicBool>,
) {
    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .expect("set relay read timeout");
    let Ok(mut ws) = tungstenite::accept(stream) else {
        return;
    };
    let mut open_subs: BTreeMap<String, Value> = BTreeMap::new();

    while !stop.load(Ordering::Relaxed) {
        if !drain_commands(&mut ws, &commands, &mut events, &open_subs) {
            return;
        }
        match ws.read() {
            Ok(Message::Text(text)) => {
                handle_client_text(&mut ws, &observed, &mut open_subs, &events, &text);
            }
            Ok(Message::Close(_)) => return,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return,
        }
    }
}

fn drain_commands(
    ws: &mut tungstenite::WebSocket<TcpStream>,
    commands: &Receiver<RelayCommand>,
    events: &mut Vec<Event>,
    open_subs: &BTreeMap<String, Value>,
) -> bool {
    loop {
        match commands.try_recv() {
            Ok(RelayCommand::Push(event)) => {
                for (sub_id, filter) in open_subs {
                    if matches_filter(&event, filter)
                        && ws
                            .send(Message::Text(json!(["EVENT", sub_id, event]).to_string()))
                            .is_err()
                    {
                        return false;
                    }
                }
                events.push(event);
                let _ = ws.flush();
            }
            Ok(RelayCommand::Stop) => return false,
            Err(std::sync::mpsc::TryRecvError::Empty) => return true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => return false,
        }
    }
}

fn handle_client_text(
    ws: &mut tungstenite::WebSocket<TcpStream>,
    observed: &Sender<ObservedFrame>,
    open_subs: &mut BTreeMap<String, Value>,
    events: &[Event],
    text: &str,
) {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return;
    };
    let Some(arr) = value.as_array() else {
        return;
    };
    match arr.first().and_then(Value::as_str) {
        Some("REQ") => {
            let Some(sub_id) = arr.get(1).and_then(Value::as_str).map(str::to_string) else {
                return;
            };
            let filter = arr.get(2).cloned().unwrap_or(Value::Null);
            open_subs.insert(sub_id.clone(), filter.clone());
            let _ = observed.send(ObservedFrame::Req(ObservedReq {
                sub_id: sub_id.clone(),
                filter: filter.clone(),
            }));
            for event in events.iter().filter(|event| matches_filter(event, &filter)) {
                if ws
                    .send(Message::Text(json!(["EVENT", sub_id, event]).to_string()))
                    .is_err()
                {
                    return;
                }
            }
            let _ = ws.send(Message::Text(json!(["EOSE", sub_id]).to_string()));
            let _ = ws.flush();
        }
        Some("CLOSE") => {
            if let Some(sub_id) = arr.get(1).and_then(Value::as_str) {
                open_subs.remove(sub_id);
                let _ = observed.send(ObservedFrame::Close {
                    sub_id: sub_id.to_string(),
                });
            }
        }
        Some("EVENT") => {
            // A client publish. Record it and ACK so the publish engine sees
            // the OK (fire-and-forget still records the frame either way).
            if let Some(ev) = arr.get(1).and_then(|v| serde_json::from_value::<Event>(v.clone()).ok())
            {
                let id = ev.id.to_hex();
                let _ = observed.send(ObservedFrame::Event(Box::new(ev)));
                let _ = ws.send(Message::Text(json!(["OK", id, true, ""]).to_string()));
                let _ = ws.flush();
            }
        }
        _ => {}
    }
}

fn matches_filter(event: &Event, filter: &Value) -> bool {
    let since = filter.get("since").and_then(Value::as_u64);
    if since.is_some_and(|since| event.created_at.as_secs() < since) {
        return false;
    }
    let until = filter.get("until").and_then(Value::as_u64);
    if until.is_some_and(|until| event.created_at.as_secs() > until) {
        return false;
    }
    let kinds = integer_set(filter.get("kinds"));
    if !kinds.is_empty() && !kinds.contains(&u64::from(event.kind.as_u16())) {
        return false;
    }
    let authors = string_set(filter.get("authors"));
    authors.is_empty() || authors.contains(&event.pubkey.to_hex())
}

pub(crate) fn has_kind(filter: &Value, kind: u64) -> bool {
    integer_set(filter.get("kinds")).contains(&kind)
}

pub(crate) fn has_author(filter: &Value, author: &str) -> bool {
    string_set(filter.get("authors")).contains(author)
}

fn string_set(value: Option<&Value>) -> BTreeSet<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn integer_set(value: Option<&Value>) -> BTreeSet<u64> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .collect()
}
