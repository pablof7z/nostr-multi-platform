use crate::kernel::Kernel;
use crate::relay::{OutboundMessage, RelayRole, DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT};
use serde_json::json;
use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Once;
use std::thread;
use std::time::{Duration, Instant};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

#[derive(Debug)]
pub(crate) enum ActorCommand {
    Start { visible_limit: usize, emit_hz: u32 },
    Configure { visible_limit: usize, emit_hz: u32 },
    OpenAuthor { pubkey: String },
    OpenThread { event_id: String },
    OpenFirehoseTag { tag: String },
    CloseAuthor { pubkey: String },
    CloseThread { event_id: String },
    Stop,
    Reset,
    Shutdown,
}

type RelaySocket = WebSocket<MaybeTlsStream<TcpStream>>;
const MAX_READS_PER_RELAY_TICK: usize = 1024;
const RELAY_IDLE_READ_TIMEOUT: Duration = Duration::from_millis(1);

pub(crate) fn run_actor(command_rx: Receiver<ActorCommand>, update_tx: Sender<String>) {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let mut sockets: HashMap<RelayRole, RelaySocket> = HashMap::new();
    let mut running = false;
    let mut emit_hz = DEFAULT_EMIT_HZ;
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let mut reconnect_after = RelayRole::all()
        .into_iter()
        .map(|role| (role, Instant::now()))
        .collect::<HashMap<_, _>>();
    let mut startup_sent = false;

    loop {
        while let Ok(command) = command_rx.try_recv() {
            let relays_ready = RelayRole::all()
                .into_iter()
                .all(|role| sockets.contains_key(&role));
            let outbound = match command {
                ActorCommand::Start {
                    visible_limit,
                    emit_hz: hz,
                } => {
                    running = true;
                    emit_hz = hz;
                    kernel.set_visible_limit(visible_limit);
                    kernel.start();
                    for role in RelayRole::all() {
                        reconnect_after.insert(role, Instant::now());
                    }
                    let _ = emit_update(&mut kernel, running, &update_tx);
                    Vec::new()
                }
                ActorCommand::Configure {
                    visible_limit,
                    emit_hz: hz,
                } => {
                    emit_hz = hz;
                    kernel.set_visible_limit(visible_limit);
                    let _ = emit_update(&mut kernel, running, &update_tx);
                    Vec::new()
                }
                ActorCommand::OpenAuthor { pubkey } => {
                    let outbound = kernel.open_author(pubkey, relays_ready);
                    let _ = emit_update(&mut kernel, running, &update_tx);
                    outbound
                }
                ActorCommand::OpenThread { event_id } => {
                    let outbound = kernel.open_thread(event_id, relays_ready);
                    let _ = emit_update(&mut kernel, running, &update_tx);
                    outbound
                }
                ActorCommand::OpenFirehoseTag { tag } => {
                    let outbound = kernel.open_firehose_tag(tag, relays_ready);
                    let _ = emit_update(&mut kernel, running, &update_tx);
                    outbound
                }
                ActorCommand::CloseAuthor { pubkey } => {
                    let outbound = kernel.close_author(&pubkey);
                    let _ = emit_update(&mut kernel, running, &update_tx);
                    outbound
                }
                ActorCommand::CloseThread { event_id } => {
                    let outbound = kernel.close_thread(&event_id);
                    let _ = emit_update(&mut kernel, running, &update_tx);
                    outbound
                }
                ActorCommand::Stop => {
                    running = false;
                    close_relays(&mut sockets, &mut kernel);
                    startup_sent = false;
                    let _ = emit_update(&mut kernel, running, &update_tx);
                    Vec::new()
                }
                ActorCommand::Reset => {
                    close_relays(&mut sockets, &mut kernel);
                    kernel = Kernel::new(kernel.visible_limit());
                    if running {
                        kernel.start();
                    }
                    for role in RelayRole::all() {
                        reconnect_after.insert(role, Instant::now());
                    }
                    startup_sent = false;
                    let _ = emit_update(&mut kernel, running, &update_tx);
                    Vec::new()
                }
                ActorCommand::Shutdown => {
                    close_relays(&mut sockets, &mut kernel);
                    return;
                }
            };

            if running {
                for message in outbound {
                    send_outbound(&mut sockets, &mut kernel, message);
                }
            }
        }

        if running {
            for role in RelayRole::all() {
                let next_attempt = reconnect_after
                    .get(&role)
                    .copied()
                    .unwrap_or_else(Instant::now);
                if !sockets.contains_key(&role) && Instant::now() >= next_attempt {
                    match open_relay(role, &mut kernel) {
                        Ok(opened) => {
                            sockets.insert(role, opened);
                            let _ = emit_update(&mut kernel, running, &update_tx);
                        }
                        Err(error) => {
                            kernel.relay_failed(role, error);
                            reconnect_after.insert(role, Instant::now() + Duration::from_secs(3));
                            startup_sent = false;
                            let _ = emit_update(&mut kernel, running, &update_tx);
                        }
                    }
                }
            }

            if !startup_sent
                && RelayRole::all()
                    .into_iter()
                    .all(|role| sockets.contains_key(&role))
            {
                for request in kernel.startup_requests() {
                    send_outbound(&mut sockets, &mut kernel, request);
                }
                for request in kernel.pending_view_requests() {
                    send_outbound(&mut sockets, &mut kernel, request);
                }
                startup_sent = true;
                let _ = emit_update(&mut kernel, running, &update_tx);
            }
        }

        if running {
            for role in RelayRole::all() {
                let Some(mut opened) = sockets.remove(&role) else {
                    continue;
                };
                let mut keep_socket = true;
                let mut outbound = Vec::new();
                for _ in 0..MAX_READS_PER_RELAY_TICK {
                    match opened.read() {
                        Ok(message) => {
                            outbound.extend(kernel.handle_message(role, message));
                            outbound.extend(kernel.pending_view_requests());
                        }
                        Err(tungstenite::Error::Io(error))
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) =>
                        {
                            break;
                        }
                        Err(error) => {
                            kernel.relay_failed(role, error.to_string());
                            reconnect_after.insert(role, Instant::now() + Duration::from_secs(3));
                            startup_sent = false;
                            keep_socket = false;
                            break;
                        }
                    }
                }
                outbound.extend(kernel.pending_view_requests());
                if keep_socket {
                    sockets.insert(role, opened);
                }
                for request in outbound {
                    send_outbound(&mut sockets, &mut kernel, request);
                }
                for request in kernel.pending_view_requests() {
                    send_outbound(&mut sockets, &mut kernel, request);
                }
            }
        }

        if running && last_emit.elapsed() >= Duration::from_secs_f64(1.0 / emit_hz as f64) {
            if kernel.changed_since_emit() {
                let _ = emit_update(&mut kernel, running, &update_tx);
            }
            last_emit = Instant::now();
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn open_relay(role: RelayRole, kernel: &mut Kernel) -> Result<RelaySocket, String> {
    install_rustls_provider();
    kernel.relay_connecting(role);
    let (mut socket, _response) = connect(role.url()).map_err(|error| error.to_string())?;
    set_read_timeout(&mut socket, RELAY_IDLE_READ_TIMEOUT);
    kernel.relay_connected(role);
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
        #[allow(unreachable_patterns)]
        _ => {}
    }
}

fn send_outbound(
    sockets: &mut HashMap<RelayRole, RelaySocket>,
    kernel: &mut Kernel,
    message: OutboundMessage,
) {
    let Some(socket) = sockets.get_mut(&message.role) else {
        kernel.defer_outbound(message);
        return;
    };

    kernel.record_tx(message.role, message.text.len());
    if let Err(error) = socket.send(Message::Text(message.text)) {
        kernel.relay_failed(message.role, error.to_string());
    }
}

fn close_relays(sockets: &mut HashMap<RelayRole, RelaySocket>, kernel: &mut Kernel) {
    for role in RelayRole::all() {
        if let Some(mut opened) = sockets.remove(&role) {
            for sub_id in kernel.active_subscriptions(role) {
                let close = json!(["CLOSE", sub_id]).to_string();
                let _ = opened.send(Message::Text(close));
            }
            let _ = opened.close(None);
        }
        kernel.relay_closed(role);
    }
}

fn emit_update(
    kernel: &mut Kernel,
    running: bool,
    update_tx: &Sender<String>,
) -> Result<(), mpsc::SendError<String>> {
    let update = kernel.make_update(running);
    update_tx.send(update)
}
