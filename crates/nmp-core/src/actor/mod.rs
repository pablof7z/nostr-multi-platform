//! Actor main loop — message routing, command dispatch, relay event handling.
//!
//! Idle-tick timing helpers are in `tick.rs`.
//! Relay lifecycle helpers are in `relay_mgmt.rs`.
//!
//! # Dual-channel priority design
//!
//! Commands (`command_rx`) are checked via `try_recv` at the top of every
//! iteration — zero latency, never dropped under relay event flood.
//! Relay events go through their own separate channel, read via
//! `recv_timeout(compute_wait(…))`. This replaces the old merged
//! `SyncSender<ActorMsg>` design where a 4096-slot bounded channel could fill
//! with relay events and cause `try_send` to silently drop commands like
//! `CreateAccount` during onboarding.

mod commands;
mod dispatch;
mod kernel_action;
mod outbound;
mod relay_mgmt;
mod tick;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod relay_url_canonical_tests;

use commands::{IdentityRuntime, WalletRuntime};
pub(crate) use commands::{
    new_event_observer_slot, new_observer_slot as new_lifecycle_observer_slot, notify_observers,
    register_c_observer, register_rust_observer, unregister_observer, KernelEventObserverSlot,
    LifecycleObserverRegistration, LifecycleObserverSlot,
};
// `pub` (not `pub(crate)`) so the `lib.rs` test-support re-export reaches
// integration tests outside the crate. The `actor` module itself is
// crate-private (`mod actor;` in `lib.rs`), so external Rust callers still
// see these only via the gated `pub use actor::{...}` in lib.rs. The
// constants are unused inside the crate (FFI consumers read them through
// the test-support facade), so allow-unused keeps a plain `cargo build`
// clean.
#[allow(unused_imports)]
pub use commands::{LifecycleObserverFn, LIFECYCLE_PHASE_BACKGROUND, LIFECYCLE_PHASE_FOREGROUND};
// T146 — re-export the kernel event observer types so external Rust callers
// (per-app crates such as `nmp-app-chirp`) can implement and register
// `KernelEventObserver`s through the gated `pub use actor::{...}` in
// `lib.rs`. The FFI shape (`KernelEventObserverFn` /
// `KernelEventObserverRegistration` / `KernelEventObserverId`) is also
// surfaced so Swift / Kotlin bindings can use the C-ABI channel.
#[allow(unused_imports)]
pub use commands::{
    KernelEventObserver, KernelEventObserverFn, KernelEventObserverId,
    KernelEventObserverRegistration,
};
use dispatch::{dispatch_command, handle_relay_event};

use crate::kernel::LifecyclePhase;

use crate::app::KernelAction;

use relay_mgmt::{
    all_relays_connected, close_relays, maybe_send_startup, send_all_outbound,
};
use tick::{compute_wait, emit_now, flush_due};

use crate::kernel::Kernel;
use crate::relay::{RelayRole, DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT};
use crate::relay_worker::{RelayCommand, RelayEvent};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicU64;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Actor command variants.  The `actor` module is private (`mod actor`, not
/// `pub mod actor`), so this `pub` is only reachable from outside the crate
/// through the `testing` re-export gate.  In normal (non-test-support) builds
/// nothing re-exports these items, so they remain effectively crate-private.
#[derive(Debug)]
pub enum ActorCommand {
    Start { visible_limit: usize, emit_hz: u32 },
    Configure { visible_limit: usize, emit_hz: u32 },
    OpenAuthor { pubkey: String },
    OpenThread { event_id: String },
    OpenFirehoseTag { tag: String },
    /// T66a identity — import an nsec/hex secret, add to the actor-local
    /// identity store, bind it as the active signer, retarget the timeline.
    SignInNsec { secret: String },
    /// T66a identity — parse a `bunker://` NIP-46 URI. Transport is NOT yet
    /// wired (D0 forbids `nmp-core -> nmp-signers`); this validates the URI
    /// shape and surfaces a `last_error_toast` directing the user to nsec.
    SignInBunker { uri: String },
    /// T66a identity — generate a fresh keypair and sign in with it.
    CreateAccount,
    /// T66a identity — switch the active account (synchronous re-bind +
    /// timeline retarget, mirrors AccountManager::switch_active semantics).
    SwitchActive { identity_id: String },
    /// T66a identity — remove an account; clears the active slot if it was
    /// the active one.
    RemoveAccount { identity_id: String },
    /// Broker → actor: register a fully-handshaken remote signer (e.g.
    /// completed NIP-46 bunker handshake). Actor inserts into
    /// `IdentityRuntime.remote_signers` and emits a snapshot update.
    /// Becomes active if no account was active. D0 stays clean — the
    /// trait object's concrete type lives in `nmp-signers` but `nmp-core`
    /// only sees `dyn RemoteSignerHandle` (defined in
    /// [`crate::remote_signer`]).
    ///
    /// Constructed by the broker crate (Stage 4) which depends on both
    /// `nmp-core` and `nmp-signers`; only test code instantiates it today.
    #[allow(dead_code)]
    AddRemoteSigner {
        handle: Box<dyn crate::RemoteSignerHandle>,
    },
    /// Broker → actor: drop a remote signer by user pubkey hex. See
    /// [`Self::AddRemoteSigner`] for the cross-crate construction story.
    #[allow(dead_code)]
    RemoveRemoteSigner { identity_id: String },
    /// Broker → actor: progress event for the bunker handshake UI. Actor
    /// stores the latest into a kernel snapshot field; the broker is the
    /// sole writer. Stage `"idle"` clears the projection. Constructed by
    /// the broker crate (Stage 4).
    #[allow(dead_code)]
    BunkerHandshakeProgress {
        /// `"connecting"` | `"awaiting_pubkey"` | `"ready"` | `"failed"` | `"idle"`.
        stage: String,
        /// Optional human-readable status (e.g. relay URL, error reason).
        message: Option<String>,
    },
    /// T66a publish — sign a kind:1 (optionally a reply) with the active
    /// account and emit it to the NIP-65 outbox-resolved write relays (D3).
    PublishNote {
        content: String,
        reply_to_id: Option<String>,
    },
    /// Generic, kind-agnostic publish — take an `UnsignedEvent` already built
    /// by any protocol-crate builder (`nmp_nip23::Article`, `nmp_nip01::Note`,
    /// `nmp_reactions::Reaction`, …), sign with the active account's keys,
    /// and route through the NIP-65 outbox resolver (D3). The kernel does
    /// not inspect the kind — that's the protocol crate's concern (D0).
    ///
    /// Stepping stone toward per-protocol-crate `ActionModule` impls
    /// (`kind-wrappers.md` §8 Phase 1); deprecates kind-by-kind as those land.
    PublishUnsignedEvent(crate::substrate::UnsignedEvent),
    /// T66a publish — kind:7 reaction to `target_event_id`.
    React {
        target_event_id: String,
        reaction: String,
    },
    /// T66a publish — append `pubkey` to the active account's kind:3 follow
    /// set and re-publish it.
    Follow { pubkey: String },
    /// T66a publish — remove `pubkey` from the kind:3 follow set.
    Unfollow { pubkey: String },
    /// T66a relay edit — add a relay row (role: `read` | `write` | `both`).
    AddRelay { url: String, role: String },
    /// T66a relay edit — remove a relay row.
    RemoveRelay { url: String },
    /// T66a — (re)open the following-timeline for the active account.
    OpenTimeline,
    ClaimProfile { pubkey: String, consumer_id: String },
    ReleaseProfile { pubkey: String, consumer_id: String },
    CloseAuthor { pubkey: String },
    CloseThread { event_id: String },
    /// NIP-47 wallet connect — parse the `nostr+walletconnect://` URI, subscribe
    /// for kind:23195 responses, and send get_info + get_balance requests.
    WalletConnect { uri: String },
    /// NIP-47 wallet disconnect — close the subscription and clear state.
    WalletDisconnect,
    /// NIP-47 pay invoice — sign and send a `pay_invoice` kind:23194 request.
    WalletPayInvoice { bolt11: String, amount_msats: Option<u64> },
    /// T118 / G3 — iOS scenePhase transition reported by the Pulse shell
    /// (or any conforming consumer). The actor folds the phase into the
    /// kernel's [`crate::kernel::LifecyclePhase`] state and, on a
    /// meaningful transition (`Background → Foreground`, `Foreground →
    /// Background`, or first phase after boot), fires the registered
    /// lifecycle observer. The observer is what fans the trigger out to
    /// `nmp_nip77::TriggerEngine` for `TriggerEvent::Foreground`; nmp-core
    /// itself does not name nip77 (D0). Idempotent: rapid scene oscillation
    /// debounces to a single observer call per transition.
    LifecycleEvent(LifecyclePhase),
    Stop,
    Reset,
    Shutdown,
    /// Generic FFI-boundary action (T95). Routed through the
    /// [`dispatch_kernel_action`] reducer; the resolved [`KernelUpdate`] is
    /// serialized and pushed on the update channel. `OpenUri` registers the
    /// resolved interest through the single-writer registry (D4).
    Kernel(KernelAction),
    /// Ingest pre-verified timeline events through the test-support kernel path.
    ///
    /// The caller is responsible for constructing `VerifiedEvent` values; this
    /// command routes each through `kernel::ingest_pre_verified_event` under the
    /// `"diag-firehose-stress"` sub-id. It inserts through the `EventStore`, then
    /// updates the lightweight read-cache directly. No signature re-verification
    /// is performed — the `VerifiedEvent` type is the gate.
    ///
    /// Test-support only (D0: not part of production FFI surface).
    #[cfg(any(test, feature = "test-support"))]
    IngestPreVerifiedEvents(Vec<crate::store::VerifiedEvent>),
    /// D6 — surface an error toast from the FFI boundary. Used when the FFI
    /// layer detects a malformed argument (e.g. unparseable JSON) and cannot
    /// call `kernel.set_last_error_toast` directly (the FFI only has a channel
    /// sender, not a kernel reference). The actor thread receives this command
    /// and routes it to `kernel.set_last_error_toast` so the error becomes
    /// observable state, never a silent no-op.
    ShowToast { message: String },
}

/// One per-URL relay-worker handle. T105: `relay_url` (NOT `role`) is the
/// pool key — every resolved write/read relay gets its own socket. `role`
/// is retained so the actor can route diagnostic-bucket updates back to
/// the kernel's lane-keyed RelayHealth rows until per-URL health lands (M11).
pub(super) struct RelayControl {
    pub(super) generation: u64,
    #[allow(dead_code)] // Diagnostic lane label; per-URL health is M11.
    pub(super) role: RelayRole,
    #[allow(dead_code)] // The URL this worker dials — the routing key in the pool.
    pub(super) relay_url: String,
    pub(super) tx: Sender<RelayCommand>,
}

use outbound::wire_frames_to_outbound;

/// Backwards-compatible entry point: spawn the actor without a lifecycle
/// observer. Existing tests and the `nmp-core::testing` facade call this
/// shape. The FFI surface uses [`run_actor_with_observers`] instead so the
/// shell can register a phase-transition callback + kernel event
/// observers.
///
/// `#[allow(dead_code)]` because callers live behind the
/// `cfg(any(test, feature = "test-support"))` gate (the `testing` facade in
/// `lib.rs` and `actor::tick`'s test module). A plain `cargo build` without
/// `--tests` or the `test-support` feature would otherwise warn.
#[allow(dead_code)]
pub fn run_actor(command_rx: Receiver<ActorCommand>, update_tx: Sender<String>) {
    run_actor_with_observers(
        command_rx,
        update_tx,
        new_lifecycle_observer_slot(),
        new_event_observer_slot(),
    );
}

/// T118 / G3 backwards-compatible entry point. Spawns the actor with a
/// lifecycle observer but no kernel event observer slot — the latter
/// defaults to an empty slot (nothing fans out, zero overhead). New
/// integrations should prefer [`run_actor_with_observers`] so kernel-event
/// fan-out is wired.
#[allow(dead_code)]
pub fn run_actor_with_lifecycle_observer(
    command_rx: Receiver<ActorCommand>,
    update_tx: Sender<String>,
    lifecycle_observer: LifecycleObserverSlot,
) {
    run_actor_with_observers(
        command_rx,
        update_tx,
        lifecycle_observer,
        new_event_observer_slot(),
    );
}

/// T118 / G3 + T146 — actor entry point that accepts BOTH the lifecycle
/// observer slot and the kernel event observer slot. The FFI
/// (`ffi/lifecycle.rs::nmp_app_set_lifecycle_callback`,
/// `ffi/event_observer.rs::nmp_app_register_event_observer`) shares the SAME
/// `Arc<Mutex<…>>` instances so registrations from outside the actor are
/// visible without crossing the FFI on each event.
///
/// Dual-channel priority design: `command_rx` is drained via `try_recv` at
/// the top of every iteration so UI commands are NEVER dropped under relay
/// event flood. Relay events use a separate channel read with
/// `recv_timeout(compute_wait(…))` so emit-hz cadence is respected.
pub fn run_actor_with_observers(
    command_rx: Receiver<ActorCommand>,
    update_tx: Sender<String>,
    lifecycle_observer: LifecycleObserverSlot,
    event_observers: KernelEventObserverSlot,
) {
    // Dual-channel design: relay events get their own dedicated channel.
    // No merged SyncSender<ActorMsg>, no forwarder threads, no drops.
    let (relay_tx, relay_rx) = mpsc::channel::<RelayEvent>();

    // T114b — bind a dispatch-drops counter for diagnostic visibility. Under
    // the new dual-channel design the counter is always zero (commands cannot
    // be dropped), but the kernel API and the Reset rebind path are kept so
    // the FFI surface and diagnostic snapshot don't change.
    let dispatch_drops = Arc::new(AtomicU64::new(0));

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // T114b — bind the FFI-channel drop counter so it surfaces on the
    // diagnostic snapshot (`Metrics::dispatch_drops_total`). A `Reset`
    // command replaces the kernel; we re-bind there so the counter stays
    // visible (the underlying `Arc<AtomicU64>` survives Reset).
    kernel.set_dispatch_drops_handle(Arc::clone(&dispatch_drops));
    // T146 — bind the shared kernel event observer slot. The kernel calls
    // `notify_event_observers` after every `EventStore::insert` returning
    // `Inserted | Replaced` (see `kernel/ingest/timeline.rs`). Per-app
    // crates (e.g. `nmp-app-chirp`) clone this slot via
    // `NmpApp::register_event_observer` to register typed observers.
    // Survives `Reset` the same way the drop counter does.
    kernel.set_event_observers_handle(Arc::clone(&event_observers));
    let mut identity = IdentityRuntime::new();
    let mut wallet = WalletRuntime::new();
    // T105: URL-keyed transport pool. One socket per resolved relay URL;
    // workers spawn on demand as OutboundMessages flow with new relay_urls.
    let mut relay_controls: HashMap<String, RelayControl> = HashMap::new();
    let mut connected_relays = HashSet::new();
    let mut connected_urls: HashSet<String> = HashSet::new(); // T116/G1 reconnect-replay discriminator.
    let mut next_relay_generation = 1;
    let mut running = false;
    let mut emit_hz = DEFAULT_EMIT_HZ;
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let mut startup_sent = false;

    loop {
        // ── Priority lane: commands ──────────────────────────────────────
        // Drain ALL pending commands before touching relay events. This is
        // the core of the dual-channel priority guarantee: commands can never
        // be starved by relay event floods because they bypass the relay_rx
        // entirely and are never queued behind relay events.
        loop {
            match command_rx.try_recv() {
                Ok(command) => {
                    let relays_ready = all_relays_connected(&connected_relays);
                    let outbound = dispatch_command(
                        command,
                        &mut kernel,
                        &mut identity,
                        &mut wallet,
                        &mut relay_controls,
                        &relay_tx,
                        &mut connected_relays,
                        &mut connected_urls,
                        &update_tx,
                        &mut last_emit,
                        &mut next_relay_generation,
                        &mut running,
                        &mut emit_hz,
                        &mut startup_sent,
                        relays_ready,
                        &lifecycle_observer,
                    );
                    let Some(outbound) = outbound else {
                        return; // Shutdown
                    };
                    if running {
                        send_all_outbound(
                            &mut relay_controls,
                            &relay_tx,
                            &mut kernel,
                            &mut next_relay_generation,
                            outbound,
                        );
                        if maybe_send_startup(
                            running,
                            &mut startup_sent,
                            &connected_relays,
                            &mut relay_controls,
                            &relay_tx,
                            &mut kernel,
                            &mut next_relay_generation,
                        ) {
                            emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    close_relays(&mut relay_controls, &mut connected_relays, &mut kernel);
                    connected_urls.clear();
                    return;
                }
            }
        }

        // ── Relay event lane ─────────────────────────────────────────────
        // Block up to compute_wait so emit-hz is respected without busy-spin.
        let wait = compute_wait(&kernel, running, last_emit, emit_hz);
        match relay_rx.recv_timeout(wait) {
            Ok(event) => {
                let relay_url = event.relay_url().to_string();
                let generation = event.generation();
                if relay_controls
                    .get(&relay_url)
                    .is_none_or(|control| control.generation != generation)
                {
                    // Stale event from a disposed worker — ignore.
                } else {
                    handle_relay_event(
                        event,
                        &mut kernel,
                        &mut wallet,
                        &mut relay_controls,
                        &relay_tx,
                        &mut next_relay_generation,
                        &mut connected_relays,
                        &mut connected_urls,
                        &update_tx,
                        &mut last_emit,
                        &mut startup_sent,
                        running,
                    );
                }
            }
            Err(_timeout_or_disconnected) => {
                // Timeout (normal idle tick) or relay_rx disconnected (actor
                // holds relay_tx so this can't happen in practice). Either way
                // fall through to idle work below.
            }
        }

        // ── Idle work (runs on every iteration after relay poll) ─────────
        // Flush any time-gated view requests (e.g. contacts_deadline).
        let pending = kernel.pending_view_requests();
        if !pending.is_empty() {
            send_all_outbound(
                &mut relay_controls,
                &relay_tx,
                &mut kernel,
                &mut next_relay_generation,
                pending,
            );
        }
        // T142 — M2 planner tick: drain the subscription lifecycle's trigger
        // inbox. Per D8, an empty inbox is a zero-cost no-op (single
        // `is_empty()` check — no allocation, no compile pass). When
        // triggers are queued (e.g. FollowListChanged A11, Nip65Arrived A1)
        // this produces REQ/CLOSE WireFrames that are converted to
        // OutboundMessages and sent to the relay pool. Placed after M1
        // `pending_view_requests()` to ensure M1 CLOSE frames are enqueued
        // before M2 opens new subs (spec §3.1 placement rationale).
        {
            let wire_frames = kernel.drain_lifecycle_tick();
            if !wire_frames.is_empty() {
                let outbound = wire_frames_to_outbound(wire_frames, &mut kernel);
                send_all_outbound(
                    &mut relay_controls,
                    &relay_tx,
                    &mut kernel,
                    &mut next_relay_generation,
                    outbound,
                );
            }
        }
        // T127: actor-tick for the publish engine. The 250ms idle poll
        // in `compute_wait` (`tick.rs`) already paces this; no
        // additional throttle (the engine's own pending_retries gate
        // skips dispatch work when nothing is due). D8 — when
        // `in_flight` is empty the tick is heap-free:
        //   - `PublishEngine::tick` collects `Vec<PublishHandle>`
        //     from an empty iterator (Rust's `FromIterator for Vec`
        //     special-cases empty → `Vec::new()`, no allocation),
        //   - `QueueDispatcher::drain` swaps in `Vec::new()` via
        //     `mem::take` (no allocation when the queue was empty),
        //   - the kernel returns `drained.into_iter().map(..).collect()`
        //     which is also heap-free for an empty source.
        // Closes Residual 1 from T117 — transient retries fire even
        // on a quiet socket (no inbound traffic).
        if running {
            let retry_frames = kernel.tick_publish_engine_for_now();
            if !retry_frames.is_empty() {
                send_all_outbound(
                    &mut relay_controls,
                    &relay_tx,
                    &mut kernel,
                    &mut next_relay_generation,
                    retry_frames,
                );
            }
        }
        // Only emit when state actually changed; do not emit on every
        // idle tick (D8: zero false-wakeup allocations after warmup).
        if flush_due(&kernel, running, last_emit, emit_hz) {
            emit_now(&mut kernel, running, &update_tx, &mut last_emit);
        }
    }
}
