use std::io::ErrorKind;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

/// Shared broadcast state: each connection owns its sender and current relay
/// subscription id, so broadcast frames carry the recipient's id.
pub(super) type BroadcastSenders = Arc<Mutex<Vec<BroadcastSender>>>;

#[derive(Clone)]
pub(super) struct BroadcastSender {
    tx: Sender<String>,
    subscription_id: Arc<Mutex<Option<String>>>,
}

/// Per-connection relay handler. Accepts REQ (registers subscription + sends
/// EOSE) and EVENT (broadcasts to all registered senders so every subscriber
/// receives the event).
pub(super) fn run_relay_connection(
    stream: std::net::TcpStream,
    shutdown: Arc<AtomicBool>,
    broadcast_senders: BroadcastSenders,
    subscription_count: Arc<AtomicUsize>,
) {
    let mut ws = match tungstenite::accept(stream) {
        Ok(w) => w,
        Err(_) => return,
    };

    let (tx, rx) = mpsc::channel::<String>();
    let subscription_id = Arc::new(Mutex::new(None::<String>));
    broadcast_senders.lock().unwrap().push(BroadcastSender {
        tx,
        subscription_id: Arc::clone(&subscription_id),
    });

    loop {
        if shutdown.load(Ordering::Relaxed) {
            let _ = ws.close(None);
            return;
        }

        while let Ok(frame) = rx.try_recv() {
            if ws.send(tungstenite::Message::Text(frame)).is_err() {
                return;
            }
        }

        let msg = match ws.read() {
            Ok(m) => m,
            Err(tungstenite::Error::Io(io_err))
                if matches!(io_err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                continue;
            }
            Err(_) => return,
        };

        let text = match msg {
            tungstenite::Message::Text(t) => t,
            tungstenite::Message::Ping(p) => {
                let _ = ws.send(tungstenite::Message::Pong(p));
                continue;
            }
            tungstenite::Message::Close(_) => return,
            _ => continue,
        };

        let parsed: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let arr = match parsed.as_array() {
            Some(a) => a,
            None => continue,
        };

        match arr.first().and_then(|v| v.as_str()).unwrap_or("") {
            "REQ" => handle_req(&mut ws, &subscription_id, &subscription_count, arr),
            "EVENT" => handle_event(&mut ws, &broadcast_senders, arr),
            "CLOSE" => return,
            _ => {}
        }
    }
}

fn handle_req(
    ws: &mut tungstenite::WebSocket<std::net::TcpStream>,
    subscription_id: &Arc<Mutex<Option<String>>>,
    subscription_count: &AtomicUsize,
    arr: &[Value],
) {
    if let Some(sub) = arr.get(1).and_then(|v| v.as_str()) {
        *subscription_id.lock().unwrap() = Some(sub.to_string());
    }
    subscription_count.fetch_add(1, Ordering::Relaxed);

    if let Some(sub) = subscription_id.lock().unwrap().as_ref() {
        let eose = json!(["EOSE", sub]).to_string();
        let _ = ws.send(tungstenite::Message::Text(eose));
    }
}

fn handle_event(
    ws: &mut tungstenite::WebSocket<std::net::TcpStream>,
    broadcast_senders: &BroadcastSenders,
    arr: &[Value],
) {
    let event = match arr.get(1) {
        Some(e) => e.clone(),
        None => return,
    };
    let event_id = event.get("id").and_then(|v| v.as_str()).unwrap_or("?");

    broadcast_event(broadcast_senders, &event);

    let ok_frame = json!(["OK", event_id, true, ""]).to_string();
    let _ = ws.send(tungstenite::Message::Text(ok_frame));
}

fn broadcast_event(broadcast_senders: &BroadcastSenders, event: &Value) {
    let senders = broadcast_senders.lock().unwrap();
    for sender in senders.iter() {
        let sub_id = sender.subscription_id.lock().unwrap().clone();
        if let Some(sub_id) = sub_id {
            let frame = json!(["EVENT", sub_id, event]).to_string();
            let _ = sender.tx.send(frame);
        }
    }
}
