use crate::kernel::Kernel;
use crate::relay::{OutboundMessage, RelayRole, DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT};
use crate::relay_worker::{spawn_relay_worker, RelayCommand, RelayEvent};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum ActorCommand {
    Start { visible_limit: usize, emit_hz: u32 },
    Configure { visible_limit: usize, emit_hz: u32 },
    OpenAuthor { pubkey: String },
    OpenThread { event_id: String },
    OpenFirehoseTag { tag: String },
    ClaimProfile { pubkey: String, consumer_id: String },
    ReleaseProfile { pubkey: String, consumer_id: String },
    CloseAuthor { pubkey: String },
    CloseThread { event_id: String },
    Stop,
    Reset,
    Shutdown,
}

enum ActorMsg {
    Command(ActorCommand),
    Relay(RelayEvent),
}

struct RelayControl {
    generation: u64,
    tx: Sender<RelayCommand>,
}

pub fn run_actor(command_rx: Receiver<ActorCommand>, update_tx: Sender<String>) {
    let (actor_tx, actor_rx) = mpsc::channel();
    bridge_commands(command_rx, actor_tx.clone());
    let (relay_tx, relay_rx) = mpsc::channel();
    bridge_relays(relay_rx, actor_tx.clone());

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let mut relay_controls: HashMap<RelayRole, RelayControl> = HashMap::new();
    let mut connected_relays = HashSet::new();
    let mut next_relay_generation = 1;
    let mut running = false;
    let mut emit_hz = DEFAULT_EMIT_HZ;
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let mut startup_sent = false;

    loop {
        let message = match next_actor_msg(&actor_rx, &kernel, running, last_emit, emit_hz) {
            Ok(Some(message)) => message,
            Ok(None) => {
                // Flush any time-gated view requests (e.g. contacts_deadline).
                let pending = kernel.pending_view_requests();
                if !pending.is_empty() {
                    send_all_outbound(&relay_controls, &mut kernel, pending);
                }
                emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                continue;
            }
            Err(()) => {
                close_relays(&mut relay_controls, &mut connected_relays, &mut kernel);
                return;
            }
        };

        match message {
            ActorMsg::Command(command) => {
                let relays_ready = all_relays_connected(&connected_relays);
                let outbound = match command {
                    ActorCommand::Start {
                        visible_limit,
                        emit_hz: hz,
                    } => {
                        running = true;
                        emit_hz = hz;
                        startup_sent = false;
                        kernel.set_visible_limit(visible_limit);
                        kernel.start();
                        spawn_missing_relays(
                            &mut relay_controls,
                            &relay_tx,
                            &mut kernel,
                            &mut next_relay_generation,
                        );
                        emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                        Vec::new()
                    }
                    ActorCommand::Configure {
                        visible_limit,
                        emit_hz: hz,
                    } => {
                        emit_hz = hz;
                        kernel.set_visible_limit(visible_limit);
                        emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                        Vec::new()
                    }
                    ActorCommand::OpenAuthor { pubkey } => {
                        let outbound = kernel.open_author(pubkey, relays_ready);
                        emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                        outbound
                    }
                    ActorCommand::OpenThread { event_id } => {
                        let outbound = kernel.open_thread(event_id, relays_ready);
                        emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                        outbound
                    }
                    ActorCommand::OpenFirehoseTag { tag } => {
                        let outbound = kernel.open_firehose_tag(tag, relays_ready);
                        emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                        outbound
                    }
                    ActorCommand::ClaimProfile {
                        pubkey,
                        consumer_id,
                    } => {
                        let outbound = kernel.claim_profile(pubkey, consumer_id, relays_ready);
                        emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                        outbound
                    }
                    ActorCommand::ReleaseProfile {
                        pubkey,
                        consumer_id,
                    } => {
                        let outbound = kernel.release_profile(&pubkey, &consumer_id);
                        emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                        outbound
                    }
                    ActorCommand::CloseAuthor { pubkey } => {
                        let outbound = kernel.close_author(&pubkey);
                        emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                        outbound
                    }
                    ActorCommand::CloseThread { event_id } => {
                        let outbound = kernel.close_thread(&event_id);
                        emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                        outbound
                    }
                    ActorCommand::Stop => {
                        running = false;
                        startup_sent = false;
                        close_relays(&mut relay_controls, &mut connected_relays, &mut kernel);
                        emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                        Vec::new()
                    }
                    ActorCommand::Reset => {
                        close_relays(&mut relay_controls, &mut connected_relays, &mut kernel);
                        kernel = Kernel::new(kernel.visible_limit());
                        startup_sent = false;
                        if running {
                            kernel.start();
                            spawn_missing_relays(
                                &mut relay_controls,
                                &relay_tx,
                                &mut kernel,
                                &mut next_relay_generation,
                            );
                        }
                        emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                        Vec::new()
                    }
                    ActorCommand::Shutdown => {
                        close_relays(&mut relay_controls, &mut connected_relays, &mut kernel);
                        return;
                    }
                };

                if running {
                    send_all_outbound(&relay_controls, &mut kernel, outbound);
                    if maybe_send_startup(
                        running,
                        &mut startup_sent,
                        &connected_relays,
                        &relay_controls,
                        &mut kernel,
                    ) {
                        emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                    }
                }
            }
            ActorMsg::Relay(event) => {
                let role = event.role();
                let generation = event.generation();
                if !relay_controls
                    .get(&role)
                    .is_some_and(|control| control.generation == generation)
                {
                    continue;
                }

                match event {
                    RelayEvent::Connected { role, .. } => {
                        connected_relays.insert(role);
                        kernel.relay_connected(role);
                        if maybe_send_startup(
                            running,
                            &mut startup_sent,
                            &connected_relays,
                            &relay_controls,
                            &mut kernel,
                        ) {
                            emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                        } else {
                            emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                        }
                    }
                    RelayEvent::Failed { role, error, .. } => {
                        connected_relays.remove(&role);
                        startup_sent = false;
                        kernel.relay_failed(role, error);
                        emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                    }
                    RelayEvent::Closed { role, .. } => {
                        connected_relays.remove(&role);
                        startup_sent = false;
                        kernel.relay_closed(role);
                        emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                    }
                    RelayEvent::Message { role, message, .. } if running => {
                        let mut outbound = kernel.handle_message(role, message);
                        outbound.extend(kernel.pending_view_requests());
                        send_all_outbound(&relay_controls, &mut kernel, outbound);
                    }
                    RelayEvent::Message { .. } => {}
                }
            }
        }

        if flush_due(&kernel, running, last_emit, emit_hz) {
            emit_now(&mut kernel, running, &update_tx, &mut last_emit);
        }
    }
}

fn bridge_commands(command_rx: Receiver<ActorCommand>, actor_tx: Sender<ActorMsg>) {
    thread::spawn(move || {
        while let Ok(command) = command_rx.recv() {
            if actor_tx.send(ActorMsg::Command(command)).is_err() {
                break;
            }
        }
    });
}

fn bridge_relays(relay_rx: Receiver<RelayEvent>, actor_tx: Sender<ActorMsg>) {
    thread::spawn(move || {
        while let Ok(event) = relay_rx.recv() {
            if actor_tx.send(ActorMsg::Relay(event)).is_err() {
                break;
            }
        }
    });
}

fn next_actor_msg(
    actor_rx: &Receiver<ActorMsg>,
    kernel: &Kernel,
    running: bool,
    last_emit: Instant,
    emit_hz: u32,
) -> Result<Option<ActorMsg>, ()> {
    if running && kernel.changed_since_emit() {
        let wait = emit_interval(emit_hz)
            .checked_sub(last_emit.elapsed())
            .unwrap_or(Duration::ZERO);
        if wait.is_zero() {
            return Ok(None);
        }
        return match actor_rx.recv_timeout(wait) {
            Ok(message) => Ok(Some(message)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(()),
        };
    }

    if running {
        // Poll at 250 ms so time-based kernel gates (e.g. contacts_deadline)
        // are checked even when no relay messages arrive.
        return match actor_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(message) => Ok(Some(message)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(()),
        };
    }

    actor_rx.recv().map(Some).map_err(|_| ())
}

fn emit_interval(emit_hz: u32) -> Duration {
    Duration::from_secs_f64(1.0 / emit_hz.max(1) as f64)
}

fn flush_due(kernel: &Kernel, running: bool, last_emit: Instant, emit_hz: u32) -> bool {
    running && kernel.changed_since_emit() && last_emit.elapsed() >= emit_interval(emit_hz)
}

fn emit_now(
    kernel: &mut Kernel,
    running: bool,
    update_tx: &Sender<String>,
    last_emit: &mut Instant,
) {
    let _ = emit_update(kernel, running, update_tx);
    *last_emit = Instant::now();
}

fn all_relays_connected(connected_relays: &HashSet<RelayRole>) -> bool {
    RelayRole::all()
        .into_iter()
        .all(|role| connected_relays.contains(&role))
}

fn spawn_missing_relays(
    relay_controls: &mut HashMap<RelayRole, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
) {
    for role in RelayRole::all() {
        if !relay_controls.contains_key(&role) {
            let generation = *next_relay_generation;
            *next_relay_generation = generation.saturating_add(1);
            kernel.relay_connecting(role);
            relay_controls.insert(
                role,
                RelayControl {
                    generation,
                    tx: spawn_relay_worker(role, generation, relay_tx.clone()),
                },
            );
        }
    }
}

fn maybe_send_startup(
    running: bool,
    startup_sent: &mut bool,
    connected_relays: &HashSet<RelayRole>,
    relay_controls: &HashMap<RelayRole, RelayControl>,
    kernel: &mut Kernel,
) -> bool {
    if !running || *startup_sent || !all_relays_connected(connected_relays) {
        return false;
    }

    let startup_requests = kernel.startup_requests();
    send_all_outbound(relay_controls, kernel, startup_requests);
    let view_requests = kernel.pending_view_requests();
    send_all_outbound(relay_controls, kernel, view_requests);
    *startup_sent = true;
    true
}

fn send_all_outbound(
    relay_controls: &HashMap<RelayRole, RelayControl>,
    kernel: &mut Kernel,
    outbound: Vec<OutboundMessage>,
) {
    for message in outbound {
        send_outbound(relay_controls, kernel, message);
    }
}

fn send_outbound(
    relay_controls: &HashMap<RelayRole, RelayControl>,
    kernel: &mut Kernel,
    message: OutboundMessage,
) {
    let Some(control) = relay_controls.get(&message.role) else {
        kernel.defer_outbound(message);
        return;
    };

    kernel.record_tx(message.role, message.text.len());
    if control.tx.send(RelayCommand::Send(message.text)).is_err() {
        kernel.relay_failed(message.role, "relay worker stopped".to_string());
    }
}

fn close_relays(
    relay_controls: &mut HashMap<RelayRole, RelayControl>,
    connected_relays: &mut HashSet<RelayRole>,
    kernel: &mut Kernel,
) {
    for role in RelayRole::all() {
        if let Some(control) = relay_controls.remove(&role) {
            for sub_id in kernel.active_subscriptions(role) {
                let close = json!(["CLOSE", sub_id]).to_string();
                let _ = control.tx.send(RelayCommand::Send(close));
            }
            let _ = control.tx.send(RelayCommand::Shutdown);
        }
        connected_relays.remove(&role);
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
