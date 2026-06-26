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

use crate::relay_protocol::{BackoffClass, KEEPALIVE_IDLE_THRESHOLD, KEEPALIVE_PONG_TIMEOUT};
use crate::relay_worker::{spawn_relay_worker_with_keepalive, RelayCommand, RelayEvent};
use crate::role::RelayRole;

use super::translate::{apply_prepared, prepare_event};
use super::types::{
    HealthState, PoolConfig, PoolEvent, PoolSnapshot, PoolSnapshotRow, RelayHandle, RelayHealth,
    RelayUrl,
};

/// Canonicalize a relay URL for slot keying, through the single workspace
/// authority [`nmp_relay_url::canonicalize`] (#967) — so the pool's slot key is
/// byte-identical to the kernel/router routing key. There is no hand-rolled
/// normalization here; the rules live solely in the Layer-0 authority.
///
/// Returns `None` (fail-closed) for an off-contract URL the authority rejects.
/// [`PoolInner::ensure_open`] then refuses to dial — it returns the dead-handle
/// sentinel rather than opening a slot under a malformed key. In production the
/// actor only hands the pool already-canonical `wss://` URLs, so this is a
/// defensive edge. (`nmp-relay-url` is Layer 0 and dependency-free, so depending
/// on it does not pull `nmp-core`.)
pub(super) fn canonicalize(raw: &str) -> Option<RelayUrl> {
    nmp_relay_url::canonicalize(raw)
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
    /// Event sink handed to [`super::Pool::new`]. An `Arc<dyn PoolEventSink>`
    /// so the actor can deliver relay events onto its unified command/relay
    /// inbox (ADR-0050 §D3a) while other callers keep handing in a plain
    /// `Sender<PoolEvent>` (blanket impl). The translator clones this handle
    /// out under the inner lock and then *drops the lock before* calling
    /// `send_event` (ADR-0050 §D3a follow-up #1231): no sink is ever invoked
    /// while the `PoolInner` mutex is held, so a sink can never stall a
    /// concurrent `Pool::send`.
    pub(super) events: Arc<dyn super::PoolEventSink>,
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

    pub(super) fn new(
        config: PoolConfig,
        events: Arc<dyn super::PoolEventSink>,
    ) -> Arc<Mutex<Self>> {
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
        // Fail-closed: a URL the canonical authority rejects gets the dead-handle
        // sentinel (same as post-shutdown) rather than a slot under a malformed
        // key — the pool never dials a non-canonicalizable relay (#967).
        let Some(canonical) = canonicalize(url) else {
            return RelayHandle {
                slot: u32::MAX,
                generation: 0,
            };
        };
        if let Some(&slot_id) = self.url_to_slot.get(&canonical) {
            if let Some(Some(state)) = self.slots.get(slot_id as usize) {
                // A live worker keeps its `command_tx`. EXCEPT after a
                // permanent failure (HTTP 401/403): the worker thread has
                // already `return`ed (see `relay_worker`), so its `command_tx`
                // is a dead channel, yet `apply_prepared` leaves the sender in
                // place while marking health `Closed`. Treating that lingering
                // sender as "live" would wedge a permanently-errored relay
                // forever — an explicit `ensure_open` (the manual "reconnect
                // all" path, #1689) could never re-dial it. So a `Closed` slot
                // always falls through to `reopen_slot`, even with a sender
                // present. The pool still does not AUTO-reconnect permanent
                // failures (no worker re-spawns itself); recovery requires an
                // explicit `ensure_open`, which this preserves.
                if state.command_tx.is_some() && state.health.state != HealthState::Closed {
                    return RelayHandle {
                        slot: slot_id,
                        generation: state.generation,
                    };
                }
            }
            // Slot exists but is closed/closing (or permanently errored) —
            // fall through to allocate a fresh generation in-place.
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

    fn reopen_slot(&mut self, slot_id: u32, canonical: RelayUrl, role: RelayRole) -> RelayHandle {
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
            self.config.user_agent.clone(),
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

    /// V-58 — deliver a one-shot [`BackoffClass`] hint to the worker for
    /// handle `h`. The worker stores the hint and applies it on the next
    /// socket disconnect; a stale handle or a closed slot is a no-op.
    /// Returns `true` iff the hint was successfully enqueued.
    pub(super) fn set_backoff_hint_for(&self, h: RelayHandle, class: BackoffClass) -> bool {
        let Some(tx) = self.command_tx_for(h) else {
            return false;
        };
        tx.send(RelayCommand::SetBackoffHint(class)).is_ok()
    }

    /// Register a reconnect preamble for the worker at handle `h`.
    ///
    /// The preamble is injected at the FRONT of the worker's outbound queue on
    /// every (re)connect, before any actor-posted commands can arrive. This is
    /// the structural REQ-before-EVENT guarantee. A stale or closed handle is a
    /// no-op. Returns `true` iff the command was enqueued.
    pub(super) fn set_reconnect_preamble_for(&self, h: RelayHandle, frames: Vec<String>) -> bool {
        let Some(tx) = self.command_tx_for(h) else {
            return false;
        };
        tx.send(RelayCommand::SetReconnectPreamble(frames)).is_ok()
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
        let (dead_worker_tx, _dead_worker_rx) = mpsc::channel();
        self.worker_event_tx = dead_worker_tx;
        // Same surgery for the public events sender. Without this, a
        // `Pool::shutdown` caller who keeps the `Pool` alive while
        // joining their own event-loop thread deadlocks: the dispatcher
        // blocks on `events_rx.recv()` waiting for the sender to drop,
        // but `PoolInner.events` is held by the `Arc<Mutex<PoolInner>>`
        // the `Pool` keeps alive, and the `Pool` won't drop until *after*
        // the caller returns from `shutdown()` and lets its `Drop` run.
        // The bunker broker's `PoolRelayClient::shutdown` hits this loop
        // exactly: it owns the `Pool` AND the dispatcher join. Dropping
        // the original `events` sender here breaks the cycle so the
        // dispatcher's `recv()` resolves once the translator finishes
        // draining post-shutdown events.
        // Replacing the boxed sink drops the original one held here; if it
        // wraps an `mpsc::Sender` (the blanket impl), that releases the
        // consumer-side `recv()` exactly as before.
        let (dead_events_tx, _dead_events_rx) = mpsc::channel::<PoolEvent>();
        self.events = Arc::new(dead_events_tx);
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
        // The potentially-expensive part of translation — converting a
        // `tungstenite::Message` into a `RelayFrame`, which JSON-parses every
        // text frame to pre-classify NIP-42 AUTH — runs HERE, fully lock-free.
        // Holding `PoolInner` across that parse would block every concurrent
        // `Pool::send` (which also takes the inner lock) for the duration of a
        // frame translate. We do it first; the lock is then only ever held for
        // the O(1) generation check + health mutation + sink enqueue.
        let prepared = match prepare_event(event) {
            Some(prepared) => prepared,
            // `Message::Frame(_)` (raw) yields no `RelayFrame`; nothing to
            // deliver. Drop without touching the lock.
            None => continue,
        };

        let Ok(mut guard) = inner.lock() else { break };
        if guard.shutdown {
            // After shutdown, keep draining so workers exit cleanly, but don't
            // push events to the (possibly already-dropped) event channel.
            continue;
        }
        // O(1) critical section: validate the generation against the slot,
        // mutate the health row, and build the `PoolEvent`.
        let pool_event = match apply_prepared(&mut guard, prepared) {
            Some(ev) => ev,
            None => continue,
        };
        // Clone the sink handle (an `Arc` bump, O(1)) and DROP the lock before
        // delivering (ADR-0050 §D3a follow-up #1231). The cross-lock-send
        // invariant is now structural rather than relying on the sink being
        // non-blocking: even if a future sink blocked, it could not stall a
        // concurrent `Pool::send`, since the `PoolInner` mutex is released
        // first. `send_event` swallows a gone-consumer error (ADR-0050 §D3a);
        // the translator stops naturally when its workers exit and the
        // `worker_event_rx.recv()` above returns `Err` on the next poll.
        let sink = Arc::clone(&guard.events);
        drop(guard);
        sink.send_event(pool_event);
    }
}

// Off-lock pre-translation (`prepare_event`) and the O(1) locked apply
// (`apply_prepared`) live in the sibling `pool::translate` module.

#[cfg(test)]
mod permanent_failure_reopen_tests {
    use super::*;
    use crate::pool::translate::PreparedEvent;

    /// #1689 — a permanently-errored slot (HTTP 401/403) must be re-openable by
    /// an explicit `ensure_open` (the manual "reconnect all" path), even though
    /// the pool does not AUTO-reconnect permanent failures.
    ///
    /// Regression guard: `apply_prepared` marks a permanent `Failed` as health
    /// `Closed` but does NOT take `command_tx` (the worker thread has already
    /// exited, so the sender is a dead channel). If `ensure_open` treated that
    /// lingering sender as "live", it would return the same handle and the
    /// relay would be wedged dead forever — `reconnect_relays` (#1689) could
    /// never recover it. This test pins that `ensure_open` reopens (bumps the
    /// generation) a `Closed` slot regardless of the lingering sender.
    #[test]
    fn ensure_open_reopens_permanently_failed_slot() {
        let (tx, _rx) = mpsc::channel::<PoolEvent>();
        let inner = PoolInner::new(PoolConfig::default(), Arc::new(tx));
        let mut guard = inner.lock().expect("lock");

        // port 1 never connects, but the slot bookkeeping is synchronous.
        let h1 = guard.ensure_open("wss://127.0.0.1:1", RelayRole::Content);
        assert_ne!(h1.slot, u32::MAX, "ensure_open must allocate a real slot");
        let canonical = canonicalize("wss://127.0.0.1:1").expect("canonical");

        // Simulate the worker emitting a PERMANENT failure (e.g. 403 Forbidden):
        // health → Closed, but command_tx is left in place by `apply_prepared`.
        let ev = apply_prepared(
            &mut guard,
            PreparedEvent::Failed {
                relay_url: canonical.as_str().to_string(),
                generation: h1.generation,
                error: "403 Forbidden".to_string(),
                permanent: true,
            },
        );
        assert!(matches!(ev, Some(PoolEvent::Failed { .. })));

        let state = guard.slots[h1.slot as usize].as_ref().expect("slot");
        assert_eq!(
            state.health.state,
            HealthState::Closed,
            "permanent failure must mark the slot Closed"
        );
        assert!(
            state.command_tx.is_some(),
            "precondition: the permanent-failure path leaves the (now dead) \
             command_tx in place — this is exactly the trap ensure_open must \
             not fall into"
        );

        // The fix: ensure_open reopens the Closed slot (fresh generation) rather
        // than returning the dead handle.
        let h2 = guard.ensure_open("wss://127.0.0.1:1", RelayRole::Content);
        assert_eq!(h2.slot, h1.slot, "reopen reuses the slot id");
        assert!(
            h2.generation > h1.generation,
            "#1689: a permanently-errored slot must reopen with a bumped \
             generation (before={}, after={})",
            h1.generation,
            h2.generation,
        );
        assert_eq!(
            guard.slots[h2.slot as usize]
                .as_ref()
                .expect("slot")
                .health
                .state,
            HealthState::Connecting,
            "reopen re-dials: health returns to Connecting"
        );

        guard.shutdown();
    }
}
