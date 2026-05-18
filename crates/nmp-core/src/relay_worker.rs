use crate::relay::RelayRole;
use std::collections::VecDeque;
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::Once;
use std::thread;
use std::time::{Duration, Instant};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

/// One physical relay-worker event.
///
/// T105: every event carries both the diagnostic `role` (the lane this URL
/// belongs to — Content/Indexer) AND the actual `relay_url` the socket
/// connects to. The url is what the URL-keyed `relay_controls` map indexes
/// on; the role is what the kernel's per-lane diagnostics use.
pub(crate) enum RelayEvent {
    Connected {
        role: RelayRole,
        relay_url: String,
        generation: u64,
    },
    Failed {
        role: RelayRole,
        relay_url: String,
        generation: u64,
        error: String,
    },
    Closed {
        role: RelayRole,
        relay_url: String,
        generation: u64,
    },
    Message {
        role: RelayRole,
        relay_url: String,
        generation: u64,
        message: Message,
    },
}

impl RelayEvent {
    #[allow(dead_code)] // Used by ingest dispatch; kept for diagnostic helpers.
    pub(crate) fn role(&self) -> RelayRole {
        match self {
            Self::Connected { role, .. }
            | Self::Failed { role, .. }
            | Self::Closed { role, .. }
            | Self::Message { role, .. } => *role,
        }
    }

    /// The URL of the relay this event originated on (T105 routing key).
    pub(crate) fn relay_url(&self) -> &str {
        match self {
            Self::Connected { relay_url, .. }
            | Self::Failed { relay_url, .. }
            | Self::Closed { relay_url, .. }
            | Self::Message { relay_url, .. } => relay_url,
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        match self {
            Self::Connected { generation, .. }
            | Self::Failed { generation, .. }
            | Self::Closed { generation, .. }
            | Self::Message { generation, .. } => *generation,
        }
    }
}

pub(crate) enum RelayCommand {
    Send(String),
    Shutdown,
}

enum RelayWorkerResult {
    Reconnect,
    Shutdown,
}

type RelaySocket = WebSocket<MaybeTlsStream<TcpStream>>;
const RELAY_READ_TIMEOUT: Duration = Duration::from_millis(50);
const RELAY_RECONNECT_DELAY: Duration = Duration::from_secs(3);

/// Spawn a worker that dials `relay_url` on transport lane `role`.
///
/// T105: the worker dials the explicit URL (the resolved write/read relay),
/// not `role.url()`. `role` is retained as the diagnostic lane label so the
/// kernel keeps per-lane RelayHealth rows while the actual sockets multiply
/// per resolved URL.
pub(crate) fn spawn_relay_worker(
    role: RelayRole,
    relay_url: String,
    generation: u64,
    relay_tx: Sender<RelayEvent>,
) -> Sender<RelayCommand> {
    let (control_tx, control_rx) = mpsc::channel();
    thread::spawn(move || run_relay_worker(role, relay_url, generation, relay_tx, control_rx));
    control_tx
}

fn run_relay_worker(
    role: RelayRole,
    relay_url: String,
    generation: u64,
    relay_tx: Sender<RelayEvent>,
    control_rx: Receiver<RelayCommand>,
) {
    let mut pending = VecDeque::new();
    loop {
        match open_relay_socket(&relay_url) {
            Ok(mut socket) => {
                if relay_tx
                    .send(RelayEvent::Connected {
                        role,
                        relay_url: relay_url.clone(),
                        generation,
                    })
                    .is_err()
                {
                    return;
                }
                match run_connected_relay(
                    role,
                    &relay_url,
                    generation,
                    &relay_tx,
                    &control_rx,
                    &mut pending,
                    &mut socket,
                ) {
                    RelayWorkerResult::Reconnect => {}
                    RelayWorkerResult::Shutdown => return,
                }
            }
            Err(error) => {
                let _ = relay_tx.send(RelayEvent::Failed {
                    role,
                    relay_url: relay_url.clone(),
                    generation,
                    error,
                });
                if !wait_before_reconnect(&control_rx, &mut pending) {
                    return;
                }
            }
        }
    }
}

fn run_connected_relay(
    role: RelayRole,
    relay_url: &str,
    generation: u64,
    relay_tx: &Sender<RelayEvent>,
    control_rx: &Receiver<RelayCommand>,
    pending: &mut VecDeque<String>,
    socket: &mut RelaySocket,
) -> RelayWorkerResult {
    loop {
        let mut shutdown = false;
        loop {
            match control_rx.try_recv() {
                Ok(RelayCommand::Send(text)) => pending.push_back(text),
                Ok(RelayCommand::Shutdown) => shutdown = true,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return RelayWorkerResult::Shutdown,
            }
        }

        if !flush_relay_writes(role, relay_url, generation, relay_tx, pending, socket) {
            return RelayWorkerResult::Reconnect;
        }
        if shutdown {
            let _ = socket.close(None);
            let _ = relay_tx.send(RelayEvent::Closed {
                role,
                relay_url: relay_url.to_string(),
                generation,
            });
            return RelayWorkerResult::Shutdown;
        }

        match socket.read() {
            Ok(message) => {
                if relay_tx
                    .send(RelayEvent::Message {
                        role,
                        relay_url: relay_url.to_string(),
                        generation,
                        message,
                    })
                    .is_err()
                {
                    return RelayWorkerResult::Shutdown;
                }
            }
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(error) => {
                let _ = relay_tx.send(RelayEvent::Failed {
                    role,
                    relay_url: relay_url.to_string(),
                    generation,
                    error: error.to_string(),
                });
                return RelayWorkerResult::Reconnect;
            }
        }
    }
}

fn flush_relay_writes(
    role: RelayRole,
    relay_url: &str,
    generation: u64,
    relay_tx: &Sender<RelayEvent>,
    pending: &mut VecDeque<String>,
    socket: &mut RelaySocket,
) -> bool {
    while let Some(text) = pending.pop_front() {
        if let Err(error) = socket.send(Message::Text(text.clone())) {
            pending.push_front(text);
            let _ = relay_tx.send(RelayEvent::Failed {
                role,
                relay_url: relay_url.to_string(),
                generation,
                error: error.to_string(),
            });
            return false;
        }
    }
    true
}

fn wait_before_reconnect(
    control_rx: &Receiver<RelayCommand>,
    pending: &mut VecDeque<String>,
) -> bool {
    let deadline = Instant::now() + RELAY_RECONNECT_DELAY;
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .unwrap_or(Duration::ZERO);
        if remaining.is_zero() {
            return true;
        }
        let wait = remaining.min(Duration::from_millis(100));
        match control_rx.recv_timeout(wait) {
            Ok(RelayCommand::Send(text)) => pending.push_back(text),
            Ok(RelayCommand::Shutdown) => return false,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return false,
        }
    }
}

fn open_relay_socket(relay_url: &str) -> Result<RelaySocket, String> {
    install_rustls_provider();
    let (mut socket, _response) = connect(relay_url).map_err(|error| error.to_string())?;
    set_read_timeout(&mut socket, RELAY_READ_TIMEOUT);
    Ok(socket)
}

fn install_rustls_provider() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn set_read_timeout(socket: &mut RelaySocket, duration: Duration) {
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => {
            let _ = stream.set_read_timeout(Some(duration));
        }
        MaybeTlsStream::Rustls(stream) => {
            let tcp = stream.get_ref();
            let _ = tcp.set_read_timeout(Some(duration));
        }
        // Stream type may have additional TLS variants in future tungstenite versions.
        #[allow(unreachable_patterns)]
        _ => {}
    }
}
