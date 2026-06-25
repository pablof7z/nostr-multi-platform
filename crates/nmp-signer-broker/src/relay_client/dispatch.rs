use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;

use nmp_network::pool::{Pool, PoolEvent, RelayFrame, RelayHandle, WireFrame};

use super::{ConnectionStateCallback, EventCallback, RelayError, BUNKER_SUB_ID};

pub(super) fn wait_for_first_open(
    events: &Receiver<PoolEvent>,
    buffered: &mut Vec<PoolEvent>,
    budget: Duration,
) -> Result<(), RelayError> {
    let deadline = std::time::Instant::now() + budget;
    loop {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .unwrap_or(Duration::ZERO);
        if remaining.is_zero() {
            return Err(RelayError::Connect(format!(
                "no relay open within {budget:?}"
            )));
        }
        match events.recv_timeout(remaining) {
            Ok(ev) => match ev {
                PoolEvent::Opened { .. } => {
                    buffered.push(ev);
                    return Ok(());
                }
                PoolEvent::Failed { ref error, .. } => {
                    return Err(RelayError::Connect(error.message.clone()));
                }
                other => buffered.push(other),
            },
            Err(RecvTimeoutError::Timeout) => {
                return Err(RelayError::Connect(format!(
                    "no relay open within {budget:?}"
                )));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(RelayError::Connect(
                    "pool translator disconnected before open".to_string(),
                ));
            }
        }
    }
}

/// Pool-event dispatcher. Blocks on `pool_events_rx` (D8: no polling) until
/// the Pool's translator drops its sender. On `Opened` replays every stored
/// subscription, on relay lifecycle events emits connection-state tokens, and
/// on text frames admits only the broker's kind-24133 EVENT envelopes.
pub(super) fn run_dispatcher(
    pool_events_rx: Receiver<PoolEvent>,
    pool: Pool,
    handle: RelayHandle,
    subscriptions: Arc<Mutex<Vec<String>>>,
    on_event: EventCallback,
    on_connection_state: Option<ConnectionStateCallback>,
    buffered: Vec<PoolEvent>,
) {
    for ev in buffered {
        handle_pool_event(
            ev,
            &pool,
            handle,
            &subscriptions,
            &on_event,
            &on_connection_state,
        );
    }
    while let Ok(ev) = pool_events_rx.recv() {
        handle_pool_event(
            ev,
            &pool,
            handle,
            &subscriptions,
            &on_event,
            &on_connection_state,
        );
    }
}

fn handle_pool_event(
    ev: PoolEvent,
    pool: &Pool,
    handle: RelayHandle,
    subscriptions: &Arc<Mutex<Vec<String>>>,
    on_event: &EventCallback,
    on_connection_state: &Option<ConnectionStateCallback>,
) {
    match ev {
        PoolEvent::Opened { .. } => {
            let frames: Vec<String> = subscriptions.lock().map(|g| g.clone()).unwrap_or_default();
            for frame in frames {
                let _ = pool.send(handle, WireFrame::Text(frame));
            }
            if let Some(cb) = on_connection_state {
                cb("connected", None);
            }
        }
        PoolEvent::Frame {
            frame: RelayFrame::Text(text),
            ..
        } => {
            if let Some(value) = parse_event_frame(&text) {
                on_event(value);
            }
        }
        PoolEvent::Frame { .. } => {}
        PoolEvent::Closed { reason, .. } => {
            if let Some(state) = closed_reason_to_state(&reason) {
                if let Some(cb) = on_connection_state {
                    cb(state, None);
                }
            }
        }
        PoolEvent::Failed { error, .. } => {
            let (state, reason) = transport_error_to_state(&error);
            if let Some(cb) = on_connection_state {
                cb(state, reason.as_deref());
            }
        }
        PoolEvent::Health { .. } => {}
    }
}

pub(crate) fn closed_reason_to_state(
    reason: &nmp_network::pool::ClosedReason,
) -> Option<&'static str> {
    use nmp_network::pool::ClosedReason;
    match reason {
        ClosedReason::Permanent => Some("failed"),
        ClosedReason::Requested => Some("reconnecting"),
        ClosedReason::Shutdown => None,
    }
}

pub(crate) fn transport_error_to_state(
    error: &nmp_network::pool::TransportError,
) -> (&'static str, Option<String>) {
    let state = if error.permanent {
        "failed"
    } else {
        "reconnecting"
    };
    (state, Some(error.message.clone()))
}

/// Parse the broker's `["EVENT", <sub_id>, <event_json>]` envelope and return
/// the `<event_json>` value. Other frame types, other subscription ids, and
/// non-kind-24133 events return `None` before they can enter broker intake.
pub(crate) fn parse_event_frame(text: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(text).ok()?;
    let arr = v.as_array()?;
    if arr.len() < 3 || arr.first()?.as_str()? != "EVENT" {
        return None;
    }
    if arr.get(1)?.as_str()? != BUNKER_SUB_ID {
        return None;
    }
    let event = arr.get(2)?.clone();
    if event.get("kind").and_then(Value::as_u64) != Some(24133) {
        return None;
    }
    Some(event)
}
