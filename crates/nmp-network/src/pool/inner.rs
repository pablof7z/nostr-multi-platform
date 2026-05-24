//! Internal state of [`super::Pool`]. Wraps the existing per-URL
//! [`crate::relay_worker::spawn_relay_worker`] lifecycle and translates
//! its [`crate::relay_worker::RelayEvent`] stream into the push-model
//! [`super::PoolEvent`] channel.
//!
//! The translator thread is the structural enforcement point for the
//! generational handle: every inbound `RelayEvent` carries the worker's
//! generation, and the translator drops any event whose generation no
//! longer matches the slot's current generation.

use std::collections::HashMap;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use tungstenite::Message;

use crate::relay_protocol::{KEEPALIVE_IDLE_THRESHOLD, KEEPALIVE_PONG_TIMEOUT};
use crate::relay_worker::{
    spawn_relay_worker_with_keepalive, RelayCommand, RelayEvent,
};
use crate::role::RelayRole;

use super::types::{
    ClosedReason, HealthState, PoolConfig, PoolEvent, PoolSnapshot, PoolSnapshotRow, RelayFrame,
    RelayHandle, RelayHealth, RelayUrl, TransportError, WireFrame,
};

/// Canonicalize a URL the same way the actor's `CanonicalRelayUrl`
/// would: trim leading/trailing whitespace, lowercase the scheme and
/// host. Kept dependency-free so `nmp-network` does not pull
/// `nmp-core`.
pub(super) fn canonicalize(raw: &str) -> RelayUrl {
    let trimmed = raw.trim();
    let lower_prefix_end = trimmed
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == ':' || c == '/' || c == '.' || c == '-'))
        .unwrap_or(trimmed.len());
    // Cheap canonicalization — full URL parser is the actor's job; the
    // pool just lowers the obvious case differences so callers that
    // hand us "WSS://Relay.Ex" and "wss://relay.ex" share a slot.
    let (head, tail) = trimmed.split_at(lower_prefix_end);
    let mut out = String::with_capacity(trimmed.len());
    out.push_str(&head.to_ascii_lowercase());
    out.push_str(tail);
    out
}

pub(super) struct SlotState {
    pub(super) url: RelayUrl,
    pub(super) role: RelayRole,
    pub(super) generation: u64,
    pub(super) command_tx: Option<Sender<RelayCommand>>,
    pub(super) health: RelayHealth,
}

pub(super) struct PoolInner {
    /// Slots indexed by dense `RelayHandle.slot` id. `None` after
    /// `Pool::close` so the slot id is reusable on the next
    /// `ensure_open` for the same URL.
    pub(super) slots: Vec<Option<SlotState>>,
    /// URL → slot id. Lookup keyed by canonicalized URL.
    pub(super) url_to_slot: HashMap<RelayUrl, u32>,
    /// Event sink handed to [`super::Pool::new`].
    pub(super) events: Sender<PoolEvent>,
    /// Worker→translator channel; one shared sender cloned to every
    /// spawned worker.
    pub(super) worker_event_tx: Sender<RelayEvent>,
    pub(super) config: PoolConfig,
    pub(super) translator: Option<JoinHandle<()>>,
    pub(super) shutdown: bool,
}

impl PoolInner {
    /// True when `Pool::shutdown` has been called.
    fn is_shutdown(&self) -> bool {
        self.shutdown
    }

    pub(super) fn new(config: PoolConfig, events: Sender<PoolEvent>) -> Arc<Mutex<Self>> {
        let (worker_event_tx, worker_event_rx) = mpsc::channel::<RelayEvent>();
        let inner = Arc::new(Mutex::new(Self {
            slots: Vec::new(),
            url_to_slot: HashMap::new(),
            events,
            worker_event_tx,
            config,
            translator: None,
            shutdown: false,
        }));
        let translator = spawn_translator(Arc::clone(&inner), worker_event_rx);
        if let Ok(mut guard) = inner.lock() {
            guard.translator = Some(translator);
        }
        inner
    }

    pub(super) fn ensure_open(&mut self, url: &str, role: RelayRole) -> RelayHandle {
        if self.is_shutdown() {
            // Post-shutdown ensure: return a sentinel handle. The
            // translator thread is gone, so no events will arrive; the
            // caller's subsequent `send` is a structural no-op.
            return RelayHandle {
                slot: u32::MAX,
                generation: 0,
            };
        }
        let canonical = canonicalize(url);
        if let Some(&slot_id) = self.url_to_slot.get(&canonical) {
            if let Some(Some(state)) = self.slots.get(slot_id as usize) {
                if state.command_tx.is_some() {
                    return RelayHandle {
                        slot: slot_id,
                        generation: state.generation,
                    };
                }
            }
            // Slot exists but is closed/closing — fall through to
            // allocate a fresh generation in-place.
            return self.reopen_slot(slot_id, canonical, role);
        }
        self.open_new_slot(canonical, role)
    }

    fn open_new_slot(&mut self, canonical: RelayUrl, role: RelayRole) -> RelayHandle {
        let slot_id = u32::try_from(self.slots.len()).expect("pool slot id overflow");
        let generation = 1;
        let command_tx = self.spawn_worker(canonical.clone(), role, generation);
        let state = SlotState {
            url: canonical.clone(),
            role,
            generation,
            command_tx: Some(command_tx),
            health: RelayHealth {
                state: HealthState::Connecting,
                ..RelayHealth::default()
            },
        };
        self.slots.push(Some(state));
        self.url_to_slot.insert(canonical, slot_id);
        RelayHandle {
            slot: slot_id,
            generation,
        }
    }

    fn reopen_slot(
        &mut self,
        slot_id: u32,
        canonical: RelayUrl,
        role: RelayRole,
    ) -> RelayHandle {
        // Bump generation off the prior value (kept on the slot when it
        // was closed) so a stale handle from before the close is
        // structurally rejected.
        let prior_generation = self
            .slots
            .get(slot_id as usize)
            .and_then(|s| s.as_ref())
            .map_or(0, |s| s.generation);
        let generation = prior_generation.saturating_add(1);
        let command_tx = self.spawn_worker(canonical.clone(), role, generation);
        let state = SlotState {
            url: canonical,
            role,
            generation,
            command_tx: Some(command_tx),
            health: RelayHealth {
                state: HealthState::Connecting,
                ..RelayHealth::default()
            },
        };
        self.slots[slot_id as usize] = Some(state);
        RelayHandle {
            slot: slot_id,
            generation,
        }
    }

    fn spawn_worker(
        &self,
        canonical: RelayUrl,
        role: RelayRole,
        generation: u64,
    ) -> Sender<RelayCommand> {
        let keepalive_idle = self
            .config
            .keepalive_idle
            .unwrap_or(KEEPALIVE_IDLE_THRESHOLD);
        let keepalive_pong = self
            .config
            .keepalive_pong_timeout
            .unwrap_or(KEEPALIVE_PONG_TIMEOUT);
        spawn_relay_worker_with_keepalive(
            role,
            canonical,
            generation,
            self.worker_event_tx.clone(),
            keepalive_idle,
            keepalive_pong,
        )
    }

    /// Returns the current command sender for `h` iff the handle is
    /// not stale. A stale handle (older generation than the slot's
    /// current generation) returns `None` — the structural rejection
    /// the spec calls for.
    pub(super) fn command_tx_for(&self, h: RelayHandle) -> Option<Sender<RelayCommand>> {
        let state = self.slots.get(h.slot as usize).and_then(|s| s.as_ref())?;
        if state.generation != h.generation {
            return None;
        }
        state.command_tx.clone()
    }

    /// Health for a (potentially stale) handle. Stale handle → `None`.
    pub(super) fn health_for(&self, h: RelayHandle) -> Option<RelayHealth> {
        let state = self.slots.get(h.slot as usize).and_then(|s| s.as_ref())?;
        if state.generation != h.generation {
            return None;
        }
        Some(state.health.clone())
    }

    pub(super) fn close(&mut self, h: RelayHandle) -> bool {
        let Some(slot) = self.slots.get_mut(h.slot as usize) else {
            return false;
        };
        let Some(state) = slot.as_mut() else {
            return false;
        };
        if state.generation != h.generation {
            return false;
        }
        if let Some(tx) = state.command_tx.take() {
            let _ = tx.send(RelayCommand::Shutdown);
        }
        state.health.state = HealthState::Closed;
        true
    }

    pub(super) fn shutdown(&mut self) {
        self.shutdown = true;
        for slot in &mut self.slots {
            let Some(state) = slot.as_mut() else { continue };
            if let Some(tx) = state.command_tx.take() {
                let _ = tx.send(RelayCommand::Shutdown);
            }
            state.health.state = HealthState::Closed;
        }
        // Drop the worker_event sender so the translator thread sees
        // `Disconnected` once every worker has exited and shuts down.
        // We can't take it out of `self` (it's used cloned), but
        // replacing with a fresh dead channel lets the original
        // senders go out of scope as workers exit.
        let (dead_tx, _dead_rx) = mpsc::channel();
        self.worker_event_tx = dead_tx;
    }

    pub(super) fn snapshot(&self) -> PoolSnapshot {
        let rows = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(slot_id, slot)| {
                let state = slot.as_ref()?;
                Some(PoolSnapshotRow {
                    handle: RelayHandle {
                        slot: slot_id as u32,
                        generation: state.generation,
                    },
                    url: state.url.clone(),
                    role: state.role,
                    health: state.health.clone(),
                })
            })
            .collect();
        PoolSnapshot { rows }
    }
}

/// Spawn the worker→pool translator thread. One thread for the whole
/// pool; it consumes the shared `worker_event_rx` and rewrites each
/// [`RelayEvent`] into a [`PoolEvent`] (dropping events whose
/// generation no longer matches the slot's current generation).
fn spawn_translator(
    inner: Arc<Mutex<PoolInner>>,
    worker_event_rx: std::sync::mpsc::Receiver<RelayEvent>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("nmp-network-pool-translator".to_string())
        .spawn(move || translator_loop(inner, worker_event_rx))
        .expect("translator thread spawn must succeed")
}

fn translator_loop(
    inner: Arc<Mutex<PoolInner>>,
    worker_event_rx: std::sync::mpsc::Receiver<RelayEvent>,
) {
    while let Ok(event) = worker_event_rx.recv() {
        let Ok(mut guard) = inner.lock() else { break };
        if guard.shutdown {
            // After shutdown, keep draining so workers exit cleanly,
            // but don't push events to the (possibly already-dropped)
            // event channel.
            continue;
        }
        let pool_event = match translate(&mut guard, event) {
            Some(ev) => ev,
            None => continue,
        };
        let events_tx = guard.events.clone();
        drop(guard);
        if events_tx.send(pool_event).is_err() {
            // Consumer dropped the receiver — no point translating
            // further events.
            break;
        }
    }
}

/// Map one [`RelayEvent`] to a [`PoolEvent`], updating the slot's
/// health row in passing. Returns `None` if the event is stale.
fn translate(inner: &mut PoolInner, event: RelayEvent) -> Option<PoolEvent> {
    let (slot_id, state) = locate_slot(inner, event.relay_url(), event.generation())?;
    let h = RelayHandle {
        slot: slot_id,
        generation: state.generation,
    };
    match event {
        RelayEvent::Connected {
            role: _,
            relay_url,
            generation,
        } => {
            state.health.state = HealthState::Connected;
            state.health.connect_count = state.health.connect_count.saturating_add(1);
            state.health.last_error = None;
            Some(PoolEvent::Opened {
                h,
                url: relay_url,
                generation,
            })
        }
        RelayEvent::Failed {
            role: _,
            relay_url: _,
            generation,
            error,
        } => {
            let permanent = crate::relay_protocol::is_permanent_error(&error);
            state.health.failure_count = state.health.failure_count.saturating_add(1);
            state.health.last_error = Some(error.clone());
            if matches!(state.health.state, HealthState::Connected) {
                state.health.state = HealthState::Reconnecting;
            }
            if permanent {
                state.health.state = HealthState::Closed;
            }
            Some(PoolEvent::Failed {
                h,
                generation,
                error: TransportError {
                    message: error,
                    permanent,
                },
            })
        }
        RelayEvent::Closed {
            role: _,
            relay_url: _,
            generation,
        } => {
            // The worker emits `Closed` only after acking a Shutdown
            // command (the normal-close path). Slot-level closes via
            // `Pool::close` surface as `ClosedReason::Requested`; the
            // `Pool::shutdown` bulk-close path short-circuits at the
            // top of `translator_loop` (no per-slot event), so we
            // never need to distinguish here.
            state.health.state = HealthState::Closed;
            Some(PoolEvent::Closed {
                h,
                generation,
                reason: ClosedReason::Requested,
            })
        }
        RelayEvent::Message {
            role: _,
            relay_url: _,
            generation,
            message,
        } => {
            let frame = tungstenite_to_relay_frame(message)?;
            Some(PoolEvent::Frame {
                h,
                generation,
                frame,
            })
        }
    }
}

/// Find the slot for `(url, generation)` and return a mutable
/// reference. Returns `None` if the slot has since been reused (the
/// stored generation is higher) or the URL was never inserted.
fn locate_slot<'a>(
    inner: &'a mut PoolInner,
    url: &str,
    generation: u64,
) -> Option<(u32, &'a mut SlotState)> {
    // The worker emits the URL it dialled (already canonicalized by
    // `ensure_open`), so the inner map's key matches byte-for-byte.
    let slot_id = *inner.url_to_slot.get(url)?;
    let state = inner.slots.get_mut(slot_id as usize)?.as_mut()?;
    if state.generation != generation {
        // Stale event — the slot has been reopened (or closed) since
        // the worker emitted this event. Drop.
        return None;
    }
    Some((slot_id, state))
}

/// Convert one `tungstenite::Message` into a [`RelayFrame`]. Returns
/// `None` for the raw `Frame(_)` variant which the kernel has never
/// observed.
fn tungstenite_to_relay_frame(message: Message) -> Option<RelayFrame> {
    match message {
        Message::Text(text) => Some(RelayFrame::Text(text)),
        Message::Binary(bytes) => Some(RelayFrame::Binary(bytes)),
        Message::Ping(_) => Some(RelayFrame::Ping),
        Message::Pong(_) => Some(RelayFrame::Pong),
        Message::Close(frame) => Some(RelayFrame::Close(frame.map(|f| f.reason.into_owned()))),
        Message::Frame(_) => None,
    }
}

/// Convert a [`WireFrame`] into the worker's `RelayCommand::Send(String)`.
/// Today only `Text` is wire-emittable; `Binary` is reserved.
pub(super) fn wire_frame_to_command(frame: WireFrame) -> Option<RelayCommand> {
    match frame {
        WireFrame::Text(text) => Some(RelayCommand::Send(text)),
        WireFrame::Binary(_) => None,
    }
}

