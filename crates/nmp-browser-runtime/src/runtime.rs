//! `BrowserRuntime` — single-writer reducer loop driven by explicit `pump()`
//! calls (issue #2046 / PR-B of the browser-runtime epic #2045).
//!
//! # Design (D4 single-writer, issue #2058)
//!
//! `BrowserRuntime` owns the `KernelReducer` and is the **sole** writer —
//! no other path mutates the kernel after `start()`. The JS host drives it
//! via `BrowserRuntimeHandle::pump()`, which drains the inbox (bounded) and
//! applies every pending `ActorCommand`, then drains the inbound relay queue,
//! runs a maintenance tick, and fans outbound frames to relay drivers.
//!
//! `BrowserRuntimeHandle` is the **only** public type (#2058): it exposes
//! the narrow dispatch / snapshot / sign surface and does NOT expose the raw
//! `KernelReducer` or `BrowserRuntime` fields, nor any mutable post-start
//! kernel handle.
//!
//! # Relay transport (#2050)
//!
//! `BrowserRuntime` owns a `RelayPool` driving WebSocket transport.
//! Inbound frames are enqueued (D4) and drained on each `pump()`.
//! Maintenance (`tick_at`) and next-deadline scheduling also run per turn.
//!
//! # File split (LOC ceiling)
//!
//! `BrowserRuntimeHandle` methods moved to `runtime/handle.rs` to keep both
//! files under the 500-LOC ceiling (AGENTS.md). New #2051/#2073/#2074/#2075
//! surface lives in `handle.rs`, `snapshot.rs`, `signer_state.rs`, and
//! `diagnostics.rs`.

use std::collections::HashMap;
use std::sync::Arc;

use nmp_core::actor::ActorMail;
use nmp_core::publish::PublishTarget;
use nmp_core::substrate::{
    PublishTrace, RelayConnectedHook, RelayTextInterceptor, RoutedRelaySet, RoutingTraceObserver,
    SubscriptionTrace,
};
use nmp_core::time::Instant;
use nmp_core::{
    ActionRegistry, CommandSender, KernelReducer, OutboundMessage, UpdateFrameBytes,
};

use crate::builder::BrowserRunConfig;
use crate::relay::RelayPool;
use crate::signer::{
    CapabilityProviderRegistry, SignerCompletion,
    SignerCompletionRx, SignerCompletionTx,
};

use std::sync::mpsc;

mod event;
pub(crate) mod pump;
mod signer_delivery;
// Pub(crate) kernel-op helpers for the wasm entry point (#2038 item A).
mod kernel_ops;
pub(crate) use kernel_ops::DispatchBytesResult;

// ── #2051/#2073 — snapshot/projection/clock/diagnostics track ────────────────
pub mod snapshot;
pub(crate) mod signer_state;
pub mod diagnostics;
mod handle;

pub use diagnostics::BrowserRuntimeDiagnostics;
pub use event::BrowserRuntimeEvent;
pub use handle::BrowserRuntimeHandle;
pub use snapshot::SnapshotOutcome;

// ── No-op routing trace observer ─────────────────────────────────────────────

pub(crate) struct NoopRoutingTrace;

impl RoutingTraceObserver for NoopRoutingTrace {
    fn on_publish(&self, _summary: PublishTrace, _routed: &RoutedRelaySet) {}
    fn on_subscription(&self, _summary: SubscriptionTrace, _routed: &RoutedRelaySet) {}
}

impl NoopRoutingTrace {
    pub(crate) fn arc() -> Arc<dyn RoutingTraceObserver> {
        Arc::new(Self)
    }
}

// ── Parked sign continuation ────────────────────────────────────────────────

/// A publish parked between `NeedsSign` and the signed-response delivery.
///
/// Mirrors `nmp-wasm`'s `PendingSignedPublish`. The runtime stores one of these
/// keyed on the sign `correlation_id` whenever a publish command needs an async
/// signature. Consumed by `signer_delivery::deliver_one_completion` when the
/// signed response arrives (broker side: `signer/completion.rs`).
#[derive(Debug, Clone)]
pub(crate) struct PendingSignedPublish {
    /// The action-level correlation id the host uses to settle the result, or
    /// `None` when the command carried none.
    pub(crate) action_correlation_id: Option<String>,
    /// The publish target resolved when the command was interpreted.
    pub(crate) target: PublishTarget,
}

// ── Pump outcome ─────────────────────────────────────────────────────────────

/// The result of one `BrowserRuntimeHandle::pump()` turn.
///
/// Carries the outbound relay frames produced, the host-facing events (sign
/// requests, command failures, relay budget exceeded), and a `yielded` flag
/// set when the per-turn command or inbound budget was exhausted (the host
/// should pump again).
#[derive(Debug, Default)]
pub struct PumpOutcome {
    /// Outbound relay frames the host's relay driver must send (already fanned
    /// to the pool's WebSocket drivers on wasm32; exposed for diagnostics /
    /// non-wasm32 consumers).
    pub outbound: Vec<OutboundMessage>,
    /// Host events emitted this turn (sign requests, command failures, budget).
    pub events: Vec<BrowserRuntimeEvent>,
    /// True when the command drain budget OR the inbound relay drain budget was
    /// hit and more mail may remain — the host should call `pump()` again.
    pub yielded: bool,
}

// ── BrowserRuntime ────────────────────────────────────────────────────────────

/// Single-writer runtime owning the `KernelReducer`. Not public — callers
/// use `BrowserRuntimeHandle`.
pub(crate) struct BrowserRuntime {
    pub(crate) reducer: KernelReducer,
    /// Action registry kept for future broker resolution of action modules.
    #[allow(dead_code)]
    pub(crate) action_registry: ActionRegistry,
    pub(crate) inbox_rx: mpsc::Receiver<ActorMail>,
    /// A sender clone stored here so relay connected-hooks can post follow-up
    /// commands back through the inbox during `pump()`.
    pub(crate) inbox_tx: mpsc::Sender<ActorMail>,
    /// Publishes parked awaiting an async signature, keyed on sign correlation
    /// id. Populated on `NeedsSign`; drained by `signer_delivery` on completion.
    pub(crate) pending_signed_publishes: HashMap<String, PendingSignedPublish>,
    /// Capability/signer-provider registry (#2049 / #2065). Populated from
    /// `BrowserBuilderInner::capability_providers` at `start()`.
    pub(crate) signer_registry: CapabilityProviderRegistry,
    /// Sender end of the sign-completion channel. Cloned into `drain_inbox`
    /// so the broker can send completions; drained in `pump()` after the
    /// command drain.
    pub(crate) signer_completion_tx: SignerCompletionTx,
    /// Receiver end of the sign-completion channel. Drained in `pump()`.
    pub(crate) signer_completion_rx: SignerCompletionRx,
    /// Relay-text interceptors — invoked during the inbound relay drain (#2050)
    /// for every inbound text frame (D0: substrate-generic, no app nouns here).
    pub(crate) relay_text_interceptors: Vec<Arc<dyn RelayTextInterceptor>>,
    /// Relay-connected hooks — invoked during the inbound relay drain (#2050)
    /// when a relay socket opens. Hooks spawn async work and return immediately
    /// (D8 no-blocking).
    pub(crate) relay_connected_hooks: Vec<Arc<dyn RelayConnectedHook>>,
    /// Identity-change callbacks (stored for future identity-change wiring).
    #[allow(dead_code)]
    pub(crate) identity_change_observers: Vec<Box<dyn Fn(Option<String>) + Send + Sync + 'static>>,
    #[allow(dead_code)]
    pub(crate) run_config: Option<BrowserRunConfig>,
    /// Browser relay pool — WebSocket drivers + inbound queue + maintenance
    /// timer (#2050). On native test builds holds only the queue and timer
    /// (no actual sockets).
    pub(crate) relay_pool: RelayPool,
    /// Events produced by `spawn_bootstrap` at start() (socket-budget exceeded,
    /// spawn-failed) that have not yet been surfaced. Drained into the first
    /// `pump()`'s `PumpOutcome.events` so a bad bootstrap relay is observable
    /// (D6 — never a silent drop).
    pub(crate) pending_startup_events: Vec<BrowserRuntimeEvent>,
}

impl BrowserRuntime {
    /// Deliver one settled sign completion to the kernel (D4: called only from
    /// `pump()` or `BrowserRuntimeHandle::deliver_signer_response`).
    fn deliver_one_completion(
        &mut self,
        completion: SignerCompletion,
    ) -> (Vec<OutboundMessage>, Vec<BrowserRuntimeEvent>) {
        signer_delivery::deliver_one_completion(
            &mut self.reducer,
            &mut self.pending_signed_publishes,
            completion,
        )
    }

    pub(crate) fn pump(&mut self) -> PumpOutcome {
        // ── 1. Command inbox drain ────────────────────────────────────────────
        // Clone the shared wake cell first (cheap Rc clone) so the async NIP-07
        // broker path can fire it from a future JS task; cloning here avoids
        // holding a borrow on the pool across the &mut reducer drain.
        let wake = self.relay_pool.wake_cell();
        let cmd_sender = CommandSender::new(self.inbox_tx.clone());
        let cmd_drain = pump::drain_inbox(
            &mut self.reducer,
            &self.inbox_rx,
            &mut self.pending_signed_publishes,
            &self.signer_registry,
            &self.signer_completion_tx,
            &wake,
        );

        // ── 1.5. Drain sign completions (bounded — same budget as cmd drain) ───
        // LocalKey completions arrive here synchronously in the same turn.
        // NIP-07 async + host-brokered completions arrive in a future turn (the
        // wake scheduled the pump that drains them).
        let mut completion_outbound: Vec<OutboundMessage> = Vec::new();
        let mut completion_events: Vec<BrowserRuntimeEvent> = Vec::new();
        let mut completions_applied = 0usize;
        let mut completion_yielded = false;
        loop {
            if completions_applied >= pump::BROWSER_COMMAND_DRAIN_BUDGET {
                // Budget hit; remaining completions stay queued. Signal a re-pump
                // so they are not stranded (mirrors the command/relay drains).
                completion_yielded = true;
                break;
            }
            match self.signer_completion_rx.try_recv() {
                Ok(c) => {
                    completions_applied += 1;
                    let (o, e) = self.deliver_one_completion(c);
                    completion_outbound.extend(o);
                    completion_events.extend(e);
                }
                Err(_) => break,
            }
        }

        // ── 2. Inbound relay event drain ──────────────────────────────────────
        let relay_drain = self.relay_pool.drain_inbound(
            &mut self.reducer,
            &self.relay_text_interceptors,
            &self.relay_connected_hooks,
            &cmd_sender,
        );

        // ── 3. Relay idle-tick sweep ──────────────────────────────────────────
        // Run every registered interceptor's `on_idle_tick` (e.g. NWC expiry
        // sweeps). Mirrors the native actor loop's idle section; D8: hooks
        // compare kernel timestamps and emit, never sleep.
        let idle_outbound = self
            .reducer
            .run_relay_idle_tick(&self.relay_text_interceptors);

        // ── 4. Maintenance tick (tick_at + arm next deadline) ─────────────────
        let now = Instant::now();
        let tick_outbound = self.relay_pool.tick_and_arm(&mut self.reducer, now);

        // ── 5. Collect all outbound from this turn ────────────────────────────
        let mut all_outbound: Vec<OutboundMessage> = Vec::new();
        all_outbound.extend(cmd_drain.outbound);
        all_outbound.extend(completion_outbound);
        all_outbound.extend(relay_drain.outbound);
        all_outbound.extend(idle_outbound);
        all_outbound.extend(tick_outbound);

        // ── 6. Fan outbound to relay drivers (wasm32: actual sends; native: no-op)
        let mut events: Vec<BrowserRuntimeEvent> = Vec::new();
        // Bootstrap spawn events (budget-exceeded / spawn-failed) captured at
        // start() are surfaced on the first pump (never silently discarded — D6).
        events.append(&mut self.pending_startup_events);
        events.extend(cmd_drain.events);
        events.extend(completion_events);
        events.extend(relay_drain.events);
        let budget_events = self.relay_pool.fan_out_outbound(&all_outbound);
        events.extend(budget_events);

        PumpOutcome {
            outbound: all_outbound,
            events,
            yielded: cmd_drain.yielded || relay_drain.yielded || completion_yielded,
        }
    }

    pub(crate) fn make_update_frame(&mut self, running: bool) -> UpdateFrameBytes {
        self.reducer.make_update_frame(running)
    }
}

#[cfg(test)]
mod tests;
