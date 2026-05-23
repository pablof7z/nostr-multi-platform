//! Low-level WebSocket I/O helpers for the relay worker thread.
//!
//! `flush_relay_writes` drains the worker's outbound queue into the
//! WebSocket's write buffer. `drain_relay_reads` reads incoming frames
//! without blocking, forwarding them to the actor's inbound channel.
//! Both functions return `FlushResult` to signal whether the socket
//! should be considered healthy or torn down.

use std::collections::VecDeque;
use std::sync::mpsc::Sender;
use std::time::Instant;

use tungstenite::Message;

use crate::keepalive::KeepaliveState;
use crate::relay::RelayRole;

use super::{is_permanent_error, RelayEvent, RelaySocket, RelayWorkerResult};

pub(super) enum FlushResult {
    Flushed,
    Blocked,
    Reconnect,
}

pub(super) fn flush_relay_writes(
    role: RelayRole,
    relay_url: &str,
    generation: u64,
    relay_tx: &Sender<RelayEvent>,
    pending: &mut VecDeque<String>,
    socket: &mut RelaySocket,
) -> FlushResult {
    while let Some(text) = pending.pop_front() {
        match socket.write(Message::Text(text.clone())) {
            Ok(()) => {}
            Err(error) if is_nonblocking_io(&error) => return FlushResult::Blocked,
            Err(error) => {
                pending.push_front(text);
                let _ = relay_tx.send(RelayEvent::Failed {
                    role,
                    relay_url: relay_url.to_string(),
                    generation,
                    error: error.to_string(),
                });
                return FlushResult::Reconnect;
            }
        }
    }
    flush_socket(socket)
}

pub(super) fn flush_socket_message(socket: &mut RelaySocket, message: Message) -> FlushResult {
    match socket.write(message) {
        Ok(()) => flush_socket(socket),
        Err(error) if is_nonblocking_io(&error) => FlushResult::Blocked,
        Err(_) => FlushResult::Reconnect,
    }
}

fn flush_socket(socket: &mut RelaySocket) -> FlushResult {
    match socket.flush() {
        Ok(()) => FlushResult::Flushed,
        Err(error) if is_nonblocking_io(&error) => FlushResult::Blocked,
        Err(_) => FlushResult::Reconnect,
    }
}

pub(super) fn drain_relay_reads(
    role: RelayRole,
    relay_url: &str,
    generation: u64,
    relay_tx: &Sender<RelayEvent>,
    socket: &mut RelaySocket,
    keepalive: &mut KeepaliveState,
) -> Option<RelayWorkerResult> {
    loop {
        match socket.read() {
            Ok(message) => {
                keepalive.on_inbound(Instant::now());
                if matches!(message, Message::Pong(_)) {
                    continue;
                }
                if relay_tx
                    .send(RelayEvent::Message {
                        role,
                        relay_url: relay_url.to_string(),
                        generation,
                        message,
                    })
                    .is_err()
                {
                    return Some(RelayWorkerResult::Shutdown);
                }
            }
            Err(error) if is_nonblocking_io(&error) => return None,
            Err(error) => {
                let error_str = error.to_string();
                let permanent = is_permanent_error(&error_str);
                let _ = relay_tx.send(RelayEvent::Failed {
                    role,
                    relay_url: relay_url.to_string(),
                    generation,
                    error: error_str,
                });
                if permanent {
                    return Some(RelayWorkerResult::PermanentFailure);
                }
                return Some(RelayWorkerResult::Reconnect);
            }
        }
    }
}

fn is_nonblocking_io(error: &tungstenite::Error) -> bool {
    matches!(
        error,
        tungstenite::Error::Io(io)
            if matches!(
                io.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
            )
    )
}
