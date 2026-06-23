//! Actor main loop — message routing, command dispatch, relay event handling.
//!
//! Idle-tick timing helpers are in `tick.rs`.
//! Relay lifecycle helpers are in `relay_mgmt.rs`.
//!
//! # Dual-channel priority design
//!
//! Commands (`command_rx`) are checked via `try_recv` at the top of every
//! iteration with a bounded burst budget — low latency, never dropped under
//! relay event flood, while relay events and idle work still progress during
//! sustained command bursts.
//! Relay events go through their own separate channel, read via
//! `recv_timeout(compute_wait(…))`. This replaces the old merged
//! `SyncSender<ActorMsg>` design where a 4096-slot bounded channel could fill
//! with relay events and cause `try_send` to silently drop commands like
//! `CreateAccount` during onboarding.

mod commands;
pub mod kind_filter;
// Tier-1 (closure-path) typed-projection codecs for the actor-owned NIP-46
// built-ins `"bunker_handshake"` / `"nip46_onboarding"`. Native-only: the
// `register_typed` registration site is in `run_actor_with_observers`
// (`#[cfg(feature = "native")]`), the only caller of these builders.
// `pub(crate)` so the decode functions (promoted from `#[cfg(test)]`) can be
// re-exported at the crate root via `typed_projections::decode_*`.
#[cfg(feature = "native")]
pub(crate) mod typed_projections;
// V-01 Phase 1c: the actor *runtime* (dispatch / tick / relay management /
// session persistence) sits on top of the native `relay_worker` and is
// therefore native-only. `ActorCommand` (pure data), the observer slots,
// and `relay_roles` (data — pure URL/role canonicalization) stay
// always-compiled below so `publish/action.rs` and every NIP-crate
// `ActionModule::execute` impl can still name `ActorCommand` without the
// `native` feature.
// V-06 / #960 — NIP-42 async-AUTH drain + obligation execution, extracted from
// the actor main loop to keep `mod.rs` within its size budget. Native-only (uses
// the native signer port + relay pool routing).
#[cfg(feature = "native")]
mod auth_sign;
#[cfg(feature = "native")]
mod builtin_projections;
#[cfg(feature = "native")]
mod capability_worker;
#[cfg(feature = "native")]
mod compat;
#[cfg(feature = "native")]
mod config;
#[cfg(feature = "native")]
mod dispatch;
// ADR-0050 §D1/§D3b signer-port dispatch helpers (cipher verbs + completion
// delivery), split out to keep `dispatch.rs` within budget. Native-only (uses
// the native `ActorContext`).
#[cfg(feature = "native")]
mod fairness;
#[cfg(feature = "native")]
mod signer_port_dispatch;
mod signer_source;
// ADR-0050 §D3a — the single waking actor inbox. `ActorMail` + `CommandSender`
// are always-compiled (the always-compiled `substrate::protocol` seam hands
// `CommandSender` to workers, and `ActorCommand` itself is always-compiled);
// the relay-side scheduler / sink / `Inbox` are `native`-gated inside.
mod inbox;
// Inbox command/relay lane priority + fairness tests, extracted from `inbox.rs`
// to keep that file under the 500 LOC hard cap (AGENTS.md).
#[cfg(all(test, feature = "native"))]
mod inbox_lane_tests;
// Always-compiled port continuations (named by the always-compiled
// `ActorCommand` sign / cipher verbs; not `native`-gated).
mod continuations;
// Generic raw signed-event forwarding dispatch. Native-only: depends on
// `nmp_network::pool::Pool` for outbound `["EVENT", ...]` frames. Policy
// crates provide target selection through a substrate trait object.
#[cfg(all(test, feature = "native"))]
mod app_managed_signer_tests;
#[cfg(all(test, feature = "native"))]
mod cipher_for_account_tests;
#[cfg(all(test, feature = "native"))]
mod nip42_async_auth_tests;
#[cfg(feature = "native")]
mod outbound;
// #1753 S6 — the parked-signer-op queue + drain is target-agnostic: the native
// actor loop drives it on idle ticks, the wasm `KernelReducer` drives it on a
// sign-completion message (pure re-entry). It must compile on all targets, so it
// is NOT `native`-gated. Its dependencies (`Kernel`, `SignerOp`, `PublishTarget`,
// `SignedEvent`, the boxed continuations) are all always-compiled.
//
// On the pure-kernel (non-`native`, i.e. wasm) build only the `SignContinuation`
// sink + `ParkedSignerOps::drive` are exercised (the wasm signing round-trip);
// the publish / auth / sign-and-return sinks and `DrainBatch`'s obligation
// fields are native-loop-only and legitimately dead there. Suppress the
// resulting dead-code warnings on that build rather than fracturing the module
// into `#[cfg]`-gated enum variants (which would force the drain's match to be
// gated too).
#[cfg_attr(not(feature = "native"), allow(dead_code))]
pub(crate) mod pending_sign;
#[cfg(all(test, feature = "native"))]
mod protocol_panic_isolation_tests;
#[cfg(all(test, feature = "native"))]
mod publish_relay_dispatch_tests;
#[cfg(feature = "native")]
pub(crate) mod raw_event_forwarder;
#[cfg(feature = "native")]
mod relay_control;
#[cfg(feature = "native")]
mod relay_event_guard;
#[cfg(feature = "native")]
mod relay_idle;
#[cfg(feature = "native")]
mod relay_mgmt;
#[cfg(feature = "native")]
mod relay_reconnect;
mod relay_roles;
#[cfg(all(test, feature = "native"))]
mod relay_url_canonical_tests;
#[cfg(all(test, feature = "native"))]
mod send_gate_universal_tests;
#[cfg(feature = "native")]
mod session_persistence;
#[cfg(all(test, feature = "native"))]
mod session_persistence_tests;
#[cfg(all(test, feature = "native"))]
mod sign_event_for_account_tests;
#[cfg(all(test, feature = "native"))]
mod signer_port_test_harness;
#[cfg(all(test, feature = "native"))]
mod tests;
#[cfg(feature = "native")]
mod tick;
#[cfg(all(test, feature = "native"))]
mod v87_d1_startup_tests;
#[cfg(all(test, feature = "native"))]
mod v90_capability_worker_tests;

// V-01 Phase 1c: capability callback and identity runtime are native actor runtime only.
#[cfg(feature = "native")]
use crate::capability_socket::new_capability_callback_slot;
#[cfg(feature = "native")]
use commands::IdentityRuntime;
// V-38: the wallet runtime + status slot moved to `crates/nmp-nip47`.
// `nmp-core` no longer has a `wallet` feature, a `WalletRuntime` use, or any
// `WalletStatusSlot` / `new_wallet_status_slot` / `WalletStatus` re-export.
// `KernelEventObserverSlot` and `notify_observers` are consumed by `kernel/event_observer.rs`
// unconditionally — keep them always-compiled. The slot constructors, registration helpers,
// and lifecycle observer types are only consumed by the native FFI and actor runtime.
pub(crate) use commands::notify_observers;
// ADR-0062: targeted observer delivery and muted-registration helpers.
// `notify_observer_by_id` is crate-internal (kernel replay path only).
// `register_rust_observer_muted` is pub so nmp-ffi can call it.
// `activate_observer` is also used by the kernel replay path, so it is
// available on wasm/no-native reducer builds.
pub use commands::activate_observer;
pub(crate) use commands::notify_observer_by_id;
pub use commands::register_rust_observer_muted;
// `KernelEventObserverSlot` and `register_rust_observer` are `pub`
// unconditionally so `nmp-ffi` and wasm32 composition roots can register
// observers. `new_event_observer_slot_headless` is `pub(crate)` — wasm32-safe
// (no drain thread); used by `KernelReducer::new` on all targets.
pub(crate) use commands::new_event_observer_slot_headless;
#[cfg(feature = "native")]
pub use commands::{
    new_event_observer_slot, new_observer_slot as new_lifecycle_observer_slot, unregister_observer,
    LifecycleObserverSlot,
};
pub use commands::{register_rust_observer, KernelEventObserverSlot};
// `register_c_observer` + `LifecycleObserverRegistration` reach `nmp-ffi`
// through `nmp_core::__ffi_internal::*` so the C-ABI bridge in
// `nmp-ffi/src/event_observer.rs` + `lifecycle.rs` can drive the slot.
#[cfg(feature = "native")]
pub use commands::{register_c_observer, LifecycleObserverRegistration};
// D0: NIP-46 remote signing is an app noun — the bunker-handshake slot is
// re-exported so the `ffi` module can build it, hand one clone to the actor's
// `IdentityRuntime`, and capture the other in the built-in
// `"bunker_handshake"` typed snapshot-projection closure.
// V-01 Phase 1c: bunker types are native actor / FFI only.
#[cfg(feature = "native")]
pub(crate) use commands::BunkerHandshakeSlot;
// `nmp-ffi`'s `nmp_app_new` constructs the bunker-handshake slot before
// handing it to the actor; promoted to `pub` for the extracted crate.
#[cfg(feature = "native")]
pub use commands::new_bunker_handshake_slot;
// ADR-0048 D6: generalised remote-signer health slot (hard-break rename of
// the former `BunkerConnectionStateSlot` — no compat aliases). The DTO itself
// stays `commands`-private; callers drive it only through the actor commands.
#[cfg(feature = "native")]
pub use commands::{new_signer_state_slot, SignerStateSlot};
pub use signer_source::SignerSource;
// `pub` (not `pub(crate)`) so the `lib.rs` test-support re-export reaches
// integration tests outside the crate. The `actor` module itself is
// crate-private (`mod actor;` in `lib.rs`), so external Rust callers still
// see these only via the gated `pub use actor::{...}` in lib.rs. The
// `lib.rs` re-export fires in two places: the test-only top-level
// (`#[cfg(any(test, feature = "test-support"))]`) and `__ffi_internal`
// (`#[cfg(feature = "native")]`). Mirror the union of those gates so the
// `pub use` is unused only in a build that consumes neither — wasm32-only
// (`--no-default-features`) without test-support.
#[cfg(any(test, feature = "test-support", feature = "native"))]
pub use commands::{LifecycleObserverFn, LIFECYCLE_PHASE_BACKGROUND, LIFECYCLE_PHASE_FOREGROUND};
// T146 — re-export the kernel event observer types so external Rust callers
// (per-app crates such as `nmp-app-chirp`) can implement and register
// `KernelEventObserver`s through the gated `pub use actor::{...}` in
// `lib.rs`. The FFI shape (`KernelEventObserverFn` /
// `KernelEventObserverRegistration` / `KernelEventObserverId`) is also
// surfaced so Swift / Kotlin bindings can use the C-ABI channel.
// `KernelEventObserver` / `KernelEventObserverFn` / `KernelEventObserverId`
// are re-exported unconditionally from `lib.rs` (the typed observer surface
// for per-app Rust crates and the FFI wire-shape). `KernelEventObserverRegistration`
// only reaches the outside world through `lib.rs::__ffi_internal`, which is
// `#[cfg(feature = "native")]`; gate the registration type re-export to match.
#[cfg(feature = "native")]
pub use commands::KernelEventObserverRegistration;
pub use commands::{KernelEventObserver, KernelEventObserverFn, KernelEventObserverId};
// `KindFilter` lives in `kind_filter.rs` (extracted so `external_event_sink`
// can use it without the raw-observer module). Re-exported unconditionally
// from `lib.rs` (used by per-app Rust crates and external_event_sink).
pub use kind_filter::KindFilter;
// NIP golden-tag conformance harness — re-exported up the (crate-private)
// `actor` chain so the gated `pub use actor::ConformanceHarness` in `lib.rs`
// reaches the `tests/nip_tag_conformance.rs` integration test. Gated on
// `test-support` so it never appears in a production build.
// V-01 Phase 1c: the harness sits on the native publish helpers, so the
// `commands` mod gates its re-export the same way; mirror the gate here.
#[cfg(all(any(test, feature = "test-support"), feature = "native"))]
pub use commands::ConformanceHarness;
// V-01 Phase 1c: every import below sits on the native actor runtime
// (`dispatch` / `fairness` / `pending_sign` / `relay_mgmt` / `tick` /
// `relay_worker`). They go away with the rest of the runtime when
// `--no-default-features` is set. `ActorCommand` (the enum below) and the
// observer types remain always-compiled — only the loop that *consumes*
// them is gated.
#[cfg(feature = "native")]
use capability_worker::spawn_capability_worker;
#[cfg(feature = "native")]
pub use config::{ActorChannels, ActorConfig, ActorConfigSources, ActorRuntimeSlots};
#[cfg(feature = "native")]
use dispatch::{dispatch_command, ActorContext};
#[cfg(feature = "native")]
use pending_sign::{ParkedSignerOps, PublishObligation};

use crate::kernel::LifecyclePhase;

use crate::app::KernelAction;

// ADR-0050 §D3a — always-compiled inbox transport types. `CommandSender` is the
// single command-send seam handed to host code, protocol/capability workers,
// the broker adapter, and the actor's self-feedback path; `ActorMail` is what
// the unified inbox carries. Both name no protocol concept (D0).
pub use inbox::{ActorMail, CommandSendError, CommandSender};
// ADR-0050 §D1 — always-compiled port continuations named by the (always-
// compiled) `ActorCommand` sign / cipher verbs.
pub use continuations::{CipherContinuation, SignContinuation};
// Native-only relay-lane scheduler + receiver wrapper. (`RelayMailSink` is
// constructed via `CommandSender::relay_sink()`, never named here.)
#[cfg(feature = "native")]
use inbox::{CommandLaneDrain, Inbox, LoopStep, MailScheduler};

#[cfg(feature = "native")]
use relay_control::RelayControl;
#[cfg(feature = "native")]
use relay_idle::{sweep_temporary_idle_relays, TEMPORARY_RELAY_IDLE_GRACE};
#[cfg(feature = "native")]
use relay_mgmt::{
    claim_send_gate, close_relays, maybe_send_startup, route_dispatch_outbound, send_all_outbound,
};
#[cfg(feature = "native")]
use tick::{compute_wait, emit_now, flush_due};

#[cfg(feature = "native")]
#[cfg(feature = "native")]
use crate::relay::{CanonicalRelayUrl, DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT};
#[cfg(feature = "native")]
// Step 8 phase F — actor cut-over to the push-model `Pool` API. The legacy
// `nmp_network::relay_worker::{RelayCommand, RelayEvent, spawn_relay_worker}`
// entry points are no longer named here; with no out-of-crate consumers
// remaining the `relay_worker` module is `pub(crate)` inside `nmp-network`
// (the `pool::Pool` translator wraps it internally). Every per-URL socket
// the actor talks to is now owned by a process-wide `Pool`; the actor
// holds a `RelayHandle` per URL in `RelayControl` and consumes `PoolEvent`s
// on the dedicated relay-event channel below.
#[cfg(feature = "native")]
#[cfg(feature = "native")]
use nmp_network::pool::{Pool, PoolConfig};
use std::collections::HashMap;
#[cfg(feature = "native")]
use std::collections::HashSet;
#[cfg(feature = "native")]
use std::sync::atomic::Ordering;
#[cfg(feature = "native")]
#[cfg(feature = "native")]
use std::sync::Arc;
#[cfg(feature = "native")]
use std::time::{Duration, Instant};

/// #1069 — interval between bounded GC passes on the actor idle tick.
///
/// `gc.md` §3: "Every 60 seconds." Gated with an `Instant`-based `last_gc`
/// local in `run_actor_with_observers` — a pure performance-timing read (like
/// `last_emit` / `TEMPORARY_RELAY_IDLE_GRACE`), never a business-logic clock,
/// so it stays D9-clean. The *event* time fed to `gc_step` is still the kernel
/// clock (`Kernel::run_gc_step` → `now_secs`); this gate only paces how often
/// the pass fires. Piggy-backs the existing ≤250 ms `compute_wait` loop wake —
/// no new sleep loop, no timer thread (D8 / AGENTS.md "no polling").
#[cfg(feature = "native")]
pub(crate) const GC_TICK_INTERVAL: Duration = Duration::from_secs(60);

// `has_role` is reached by `nmp-ffi` through
// `nmp_core::__ffi_internal::has_role` (the FFI surface filters relay-edit
// rows by role when computing the write-relay slice for the per-app crate's
// MLS / NIP-17 publish path).
pub use relay_roles::has_role;
pub(crate) use relay_roles::{canonical_relay_role, relay_role_options};
// V6 Stage 1 — Swift codegen pilot. `RelayRoleOption` is `pub(crate)` in
// `relay_roles`; re-exported here so `crate::codegen_schema` can hand it
// to `schemars::schema_for!` from the schema-dump binary. The type stays
// crate-private; the re-export is `pub(crate)`, the bin runs inside the
// crate. Gated to the codegen-schema build so non-codegen builds don't
// trip the unused-import lint (no in-crate consumer outside codegen_schema).
#[cfg(feature = "codegen-schema")]
pub(crate) use relay_roles::RelayRoleOption;
// `nostrconnect_relay_url` is consumed by `nmp-ffi` (native only) through
// `nmp_core::__ffi_internal::nostrconnect_relay_url`.
#[cfg(feature = "native")]
pub use relay_roles::nostrconnect_relay_url;

// ADR-0065 — `ActorCommand` collapsed into typed command-family payloads.
// The top-level enum + sub-enum families live in `actor_command.rs`; the
// `SignerSource` type lives in `signer_source.rs` (extracted by #1903). Both
// are re-exported here so existing `use crate::actor::ActorCommand` /
// `use crate::SignerSource` paths keep working.
mod actor_command;
pub use actor_command::{
    ActionLedgerCommand, ActorCommand, ContactsCommand, IdentityCommand, InterestsCommand,
    LifecycleCommand, PublishCommand, RelayCommand, RefsCommand, SignCommand,
};
#[cfg(any(test, feature = "test-support"))]
pub use actor_command::TestSupportCommand;

// ─────────────────────────────────────────────────────────────────────────────
// V-01 Phase 1c: the actor runtime — per-URL relay handles, the public
// entry points (`run_actor*`), and every loop / dispatch helper below —
// sits on top of the native `relay_worker`. Gated behind `native` so the
// crate compiles without the WebSocket transport. Everything above (the
// `ActorCommand` enum, observer types, `relay_roles`) stays always-compiled.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "native")]
use outbound::wire_frames_to_outbound;

#[cfg(all(feature = "native", any(test, feature = "test-support")))]
pub use compat::run_actor;
#[cfg(feature = "native")]
#[allow(unused_imports)]
pub use compat::run_actor_with_lifecycle_observer;

/// T118 / G3 + T146 — actor entry point that accepts BOTH the lifecycle
/// observer slot and the kernel event observer slot. The FFI
/// (`ffi/lifecycle.rs::nmp_app_set_lifecycle_callback`,
/// `ffi/event_observer.rs::nmp_app_register_event_observer`) shares the SAME
/// `Arc<Mutex<…>>` instances so registrations from outside the actor are
/// visible without crossing the FFI on each event.
///
/// Single-inbox priority design (ADR-0050 §D3a): `inbox_rx` carries both
/// commands and relay events as [`ActorMail`]. Each iteration drains the
/// command lane via `try_recv` first (budgeted, stashing any relay mail seen
/// along the way), then makes the loop's single blocking `recv_timeout` — so a
/// command send wakes a relay-blocked actor instead of waiting out the 250 ms
/// idle cap. Command-lane priority and the [`COMMAND_DRAIN_BUDGET`] fairness
/// budget are preserved exactly; relay events still surface at emit-hz cadence
/// when the command lane is not saturated.
#[cfg(feature = "native")]
pub fn run_actor_with_observers(
    channels: ActorChannels,
    config: ActorConfig,
    runtime: ActorRuntimeSlots,
) {
    let ActorChannels {
        inbox_rx,
        command_tx_self,
        update_tx,
    } = channels;
    let ActorRuntimeSlots {
        lifecycle_observer,
        event_observers,
        snapshot_projections,
        bunker_handshake,
        signer_state,
        bunker_hook,
        external_signer_hook,
        configured_relays,
        mls_local_nsec,
        active_local_keys,
        capability_callback,
        queue_depth,
        routing_trace,
        active_account,
        event_store,
        pull_cursor_registry,
        external_event_sink_dispatcher: dispatcher_slot,
    } = runtime;
    // Dual-channel design: relay events get their own dedicated channel.
    // No merged SyncSender<ActorMsg>, no forwarder threads, no drops.
    //
    // Phase F: the channel item is now [`PoolEvent`] (push-model surface from
    // `nmp_network::pool`). The `Pool` is constructed eagerly here — it owns
    // every per-URL worker thread and the worker→pool translator thread that
    // rewrites `RelayEvent` into `PoolEvent`. Default `PoolConfig` (production
    // keepalive constants, `RelayRole::Content` default lane) matches the
    // pre-Pool actor behaviour bit-for-bit; per-URL role attribution still
    // flows through `Pool::ensure_open_with_role` from `ensure_relay_worker`.
    // ADR-0050 §D3a — the pool delivers relay events through a
    // `RelayMailSink` that wraps each `PoolEvent` into `ActorMail::Relay` and
    // pushes it onto the SAME inbox `inbox_rx` receives commands on. There is
    // no longer a separate `relay_rx`: relay traffic and commands share one
    // waking channel, so a command send wakes a relay-blocked actor.
    let inbox = Inbox::new(inbox_rx);
    let pool = Pool::new(PoolConfig::default(), command_tx_self.relay_sink());

    // The lane scheduler (ADR-0050 §D3a). It owns the relay backlog so any
    // relay mail stashed while draining the command lane each iteration is
    // replayed in order.
    let mut scheduler = MailScheduler::new();

    // The actor owns the only live kernel. FFI/app configuration was snapped at
    // `nmp_app_start` into `config`; runtime-observable handles stay in
    // `runtime` so registrations and publish-back slots preserve identity.
    let mut kernel =
        config.kernel_with_account_slot(DEFAULT_VISIBLE_LIMIT, Arc::clone(&active_account));
    if let Ok(mut guard) = routing_trace.lock() {
        *guard = Some(kernel.routing_trace());
    }
    if let Ok(mut guard) = event_store.lock() {
        *guard = Some(kernel.event_store_handle());
    }
    // ADR-0058 step 3b — publish the kernel's pull-cursor registry handle so the
    // synchronous FFI `pull_page` path can snapshot a registration. Re-published
    // on `Reset` (see dispatch.rs) the same way the event-store handle is.
    if let Ok(mut guard) = pull_cursor_registry.lock() {
        *guard = Some(kernel.pull_cursor_registry_handle());
    }
    config.apply_to_kernel(&mut kernel);
    // G-S4 — bind the actor command-channel depth counter so it surfaces on
    // the diagnostic snapshot (`Metrics::actor_queue_depth`). `NmpApp::send_cmd`
    // increments it; this loop decrements per dequeued command (both recv
    // sites below). Survives `Reset` the same way the drop counter does —
    // re-bound there so the counter stays visible across a kernel rebuild.
    kernel.set_queue_depth_handle(Arc::clone(&queue_depth));
    // T146 — bind the shared kernel event observer slot. The kernel calls
    // `notify_event_observers` after every `EventStore::insert` returning
    // `Inserted | Replaced` (see `kernel/ingest/timeline.rs`). Per-app
    // crates (e.g. `nmp-app-chirp`) clone this slot via
    // `NmpApp::register_event_observer` to register typed observers.
    // Survives `Reset` the same way the drop counter does.
    kernel.set_event_observers_handle(Arc::clone(&event_observers));
    // The ExternalEventSinkDispatcher replaces the raw-event-forwarder +
    // pool-send inline path.  The dispatcher owns a bounded channel + worker
    // thread (off the actor thread).  Policies are set via
    // `register_raw_event_forward_policies_from_factory` below and re-installed
    // after every `Reset`.
    //
    // Instance-identity fix: the dispatcher exists from app construction
    // (zero-arg `new()`), so the FFI layer may already have published an
    // instance into `dispatcher_slot` before this actor thread spawned. Adopt
    // that published instance if present so the actor and any FFI handle share
    // one dispatcher. Only if the slot is empty (non-FFI test harnesses) do we
    // create + publish one.
    let external_event_sink_dispatcher = {
        let existing = dispatcher_slot.lock().ok().and_then(|guard| guard.clone());
        match existing {
            Some(d) => d,
            None => {
                let d = crate::substrate::ExternalEventSinkDispatcher::new();
                if let Ok(mut guard) = dispatcher_slot.lock() {
                    *guard = Some(d.clone());
                }
                d
            }
        }
    };
    // Bind the live Pool and spawn the worker thread. Any frames that arrived
    // before this point are retained on the bounded channel and processed as
    // soon as the worker starts.
    external_event_sink_dispatcher.bind_runtime(pool.clone());
    // Bind the dispatcher to the kernel so `persistence.rs` can dispatch
    // frames from the single all-kinds ingest chokepoint.
    kernel.set_external_event_sink_dispatcher(external_event_sink_dispatcher.clone());
    // Raw signed-event forwarding policies are installed through a
    // substrate factory.  The actor contributes only the live kernel
    // handles; target selection and dedup live in the injected policy crate.
    raw_event_forwarder::register_raw_event_forward_policies_from_factory(
        &kernel,
        &external_event_sink_dispatcher,
        config.external_event_sink_policy.clone(),
    );
    // Bind the shared snapshot-projection slot. The kernel runs every
    // host-registered projection closure in `make_update` and appends the
    // result to `KernelSnapshot::projections`. Per-app crates register
    // through the C-ABI `nmp_app_register_snapshot_projection`, which mutates
    // the same `Arc<Mutex<…>>`. Survives `Reset` the same way the other
    // shared handles do so host projections stay live across a kernel
    // rebuild.
    kernel.set_snapshot_projection_handle(Arc::clone(&snapshot_projections));
    builtin_projections::register_builtin_projections(
        &snapshot_projections,
        &bunker_handshake,
        &signer_state,
    );
    // Bind the shared relay-edit rows handle so external Rust callers
    // (e.g. a per-app dispatch crate) can read the user's current
    // relay list without crossing FFI. Survives `Reset` the same way as
    // the other shared handles.
    kernel.set_app_relay_slot(Arc::clone(&configured_relays));
    // D4: the identity runtime is the sole writer of the shared
    // bunker-handshake slot. The built-in `"bunker_handshake"` snapshot
    // projection registered above reads the same `Arc<Mutex<…>>` clone on
    // every tick. Same for `signer_state` (ADR-0048 D6).
    let mut identity = IdentityRuntime::new(bunker_handshake, signer_state);
    // ADR-0052 §D3 — bind the per-app signer hook slots so the FFI broker /
    // NIP-55 driver install into the SAME slots this runtime reads.
    identity.set_signer_hook_slots(bunker_hook, external_signer_hook);
    // V-38: the wallet runtime moved to `nmp-nip47`. The actor no longer
    // owns it; the substrate relay-text interceptor slot
    // (`relay_text_interceptor`) is the only seam the actor calls for NIP-47
    // NWC behavior.
    let mut running = false;
    let mut emit_hz = DEFAULT_EMIT_HZ;
    let mut last_emit = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);
    // D1 / offline-first §3 — emit one empty-but-valid snapshot from the real
    // kernel before the actor processes any command. Because the same kernel
    // later handles `Start`, a snapshot-first host sees strict rev monotonicity
    // naturally: the initial `running=false` frame is rev=1 and the first
    // `running=true` Start frame is rev=2.
    emit_now(&mut kernel, running, &update_tx, &mut last_emit);

    // T105: URL-keyed transport pool. One socket per resolved relay URL;
    // workers spawn on demand as OutboundMessages flow with new relay_urls.
    // Keyed by `CanonicalRelayUrl` so the canonicalization invariant is
    // compiler-enforced — a raw `&str` cannot index the pool.
    let mut relay_controls: HashMap<CanonicalRelayUrl, RelayControl> = HashMap::new();
    // Phase F: reverse lookup from a `RelayHandle.slot()` back to the
    // canonical pool key. Inbound `PoolEvent`s carry the handle but not the
    // URL on every variant (`Opened` carries it; `Frame`/`Closed`/`Failed`
    // do not), so we maintain this side-map alongside `relay_controls` so
    // the event dispatcher can resolve `slot → (url, role)` without an
    // O(n) scan. Inserted by `ensure_relay_worker`, removed by
    // `shutdown_relay_worker` / `close_relays`.
    let mut slot_to_url: HashMap<u32, CanonicalRelayUrl> = HashMap::new();
    let mut connected_relays = HashSet::new();
    let mut connected_urls: HashSet<CanonicalRelayUrl> = HashSet::new(); // T116/G1 reconnect-replay discriminator.
    let mut next_relay_generation = 1;
    // #1069 — wall-clock gate for the bounded GC pass. Initialised to "now" so
    // the first pass fires one `GC_TICK_INTERVAL` after the actor starts, not
    // on the cold-start burst (the store is empty then anyway). An `Instant`
    // (performance-timing) read, never the business clock — D9-clean.
    let mut last_gc = Instant::now();
    let mut startup_sent = false;
    // The single unified parked-op queue (ADR-0050 §D2; #1753). `dispatch_command`
    // pushes a `ParkedOp` whenever a remote (NIP-46 / NIP-55) signer goes
    // `Pending` — publish, sign-and-return, the generic sign port, and the
    // cipher port (§D1) all land here and are drained in ONE `drive` below.
    // `ParkedSignerOps` is the target-agnostic queue + drain driver shared with
    // the wasm `KernelReducer` (#1753) so there is one drain, not a parallel copy.
    // Lives outside the loop so parked ops survive across ticks.
    let mut parked_ops = ParkedSignerOps::new();
    let mut queued_publish_outbound = Vec::new();
    let mut first_command = None;

    // ADR-0040 §3 — spawn the serialized capability-worker thread (V-90 Site 2).
    // The worker owns the Receiver; the actor holds `capability_work_tx` and
    // hands borrows of it to `ActorContext` on each dispatch. Dropping
    // `capability_work_tx` on actor teardown closes the channel and the worker
    // exits its blocking `recv` loop cleanly (D8).
    let capability_work_tx =
        spawn_capability_worker(Arc::clone(&capability_callback), command_tx_self.clone());

    loop {
        // ── Priority lane: commands ──────────────────────────────────────
        // Drain a bounded burst of pending commands before touching relay
        // events. Commands still get first service on every iteration, but the
        // budget prevents a sustained command stream from starving relay
        // events, subscription ticks, publish retries, and parked sign ops.
        // Single drain (issue #1231 follow-up #3): `MailScheduler::
        // drain_command_lane` is now the *only* implementation of the
        // command-priority + fairness + relay-backlog contract. It replays the
        // command held from the prior blocking wait, drains up to
        // `COMMAND_DRAIN_BUDGET` commands, stashes any relay mail it sees
        // (honoring the #1264 RELAY_BACKLOG_CAP backpressure: once the backlog
        // is full it STOPS pulling relay mail forward, leaving it in the
        // bounded mpsc channel so pressure builds at the pool translator
        // rather than silently dropping the oldest staged event), and returns
        // the commands as a `Vec` so the `&mut kernel` / `&mut identity`
        // per-command dispatch (which a closure boundary cannot express,
        // hence the prior inline copy) runs here, after the drain returns.
        let CommandLaneDrain {
            commands,
            drain: command_drain,
            disconnected: inbox_disconnected,
        } = scheduler.drain_command_lane(&inbox, first_command.take());
        for command in commands {
            {
                {
                    // G-S4 — straddle counter: one command has left the channel
                    // through `drain_command_lane`. Mirror `NmpApp::send_cmd`'s
                    // `fetch_add(1)` so the depth tracks occupancy.
                    // `saturating_sub` guards the (benign) race where the actor
                    // drains a command sent through `actor_sender`, which
                    // bypasses the increment. `Relaxed` — observability, not
                    // synchronization.
                    queue_depth
                        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |d| {
                            Some(d.saturating_sub(1))
                        })
                        .ok();
                    // Bundle the actor's mutable runtime state into a borrowed
                    // `ActorContext` for the duration of this one dispatch.
                    // Built fresh per command and dropped immediately after, so
                    // every other call site in this loop keeps using the
                    // original locals untouched (no loop-lifetime borrow).
                    //
                    // Fix A (universal latent-bug fix): `relays_ready` is the
                    // SINGLE claim/open send-gate, computed here once per dispatch
                    // and fed to every consumer (claim_event / resolve_ref /
                    // open_author / open_thread / open_firehose /
                    // sign_in_nsec→retarget / session restore). `claim_send_gate`
                    // returns true as soon as ANY bootstrap lane is connected; the
                    // prior `all`-lane gate parked every claim forever when one
                    // lane (e.g. the Indexer) never opened its socket. See
                    // `relay_mgmt::claim_send_gate` for the full rationale and the
                    // proof that hosts connecting all lanes (iOS/TUI) are
                    // behavior-preserved.
                    let relays_ready = claim_send_gate(&connected_relays);
                    let mut ctx = ActorContext {
                        kernel: &mut kernel,
                        identity: &mut identity,
                        relay_controls: &mut relay_controls,
                        slot_to_url: &mut slot_to_url,
                        pool: &pool,
                        connected_relays: &mut connected_relays,
                        connected_urls: &mut connected_urls,
                        update_tx: &update_tx,
                        last_emit: &mut last_emit,
                        next_relay_generation: &mut next_relay_generation,
                        running: &mut running,
                        emit_hz: &mut emit_hz,
                        startup_sent: &mut startup_sent,
                        relays_ready,
                        lifecycle_observer: &lifecycle_observer,
                        mls_local_nsec: &mls_local_nsec,
                        active_local_keys: &active_local_keys,
                        capability_callback: &capability_callback,
                        parked_ops: &mut parked_ops,
                        command_tx_self: &command_tx_self,
                        capability_work_tx: &capability_work_tx,
                        config: &config,
                        routing_trace_slot: &routing_trace,
                        event_store_slot: &event_store,
                        pull_cursor_registry_slot: &pull_cursor_registry,
                        active_account_slot: &active_account,
                        external_event_sink_dispatcher: &external_event_sink_dispatcher,
                    };
                    let outbound = dispatch_command(command, &mut ctx);
                    let Some(outbound) = outbound else {
                        return; // Shutdown
                    };
                    route_dispatch_outbound(
                        running,
                        &mut queued_publish_outbound,
                        &mut relay_controls,
                        &mut slot_to_url,
                        &pool,
                        &mut kernel,
                        &mut next_relay_generation,
                        outbound,
                    );
                    if running
                        && maybe_send_startup(
                            running,
                            &mut startup_sent,
                            &connected_relays,
                            &mut relay_controls,
                            &mut slot_to_url,
                            &pool,
                            &mut kernel,
                            &mut next_relay_generation,
                        )
                    {
                        emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                    }
                }
            }
        }
        // Inbox closed (every `CommandSender` clone dropped) → tear down. This
        // is the merged-inbox equivalent of the old `command_rx`
        // `Disconnected` arm: relay traffic alone can never disconnect the
        // inbox (the actor holds the relay sink), so a disconnect means all
        // command senders are gone.
        if inbox_disconnected {
            close_relays(
                &mut relay_controls,
                &mut slot_to_url,
                &pool,
                &mut connected_relays,
                &mut kernel,
            );
            connected_urls.clear();
            return;
        }

        // ── Relay event lane ─────────────────────────────────────────────
        // Block up to compute_wait so emit-hz is respected without busy-spin.
        // This `recv_timeout` is the loop's SINGLE blocking point (D8): a
        // backlog relay event (stashed while draining commands) is served
        // first with zero wait; otherwise we block on the unified inbox, so a
        // command send wakes us here too. A command
        // received during the wait is replayed as `first_command` so the next
        // iteration dispatches it on the priority lane (no added latency).
        //
        // Phase F: the inbound item is `PoolEvent` (push-model). Stale-event
        // filtering moved into `handle_relay_event` itself — the helper
        // resolves `RelayHandle.slot()` → `(url, role)` via the
        // `slot_to_url` side-map and the `relay_controls` entry, dropping
        // any handle whose generation no longer matches the slot's current
        // generation. The pool's translator already drops events with a
        // stale slot-generation, so this is belt-and-braces.
        // Relay events are processed under panic isolation — see
        // `relay_event_guard::process_relay_event`. `handle_relay_event`
        // parses arbitrary network bytes (the highest-risk panic site in the
        // actor); the guard's `catch_unwind` keeps a panic from killing the
        // loop (D1: partial state tolerated, loop survival is the invariant).
        // The same guarded helper serves BOTH the bounded backlog batch and
        // the single recv'd event below (#1264).
        //
        // A small local macro forwards the actor's ~13 loop locals into the
        // helper from both call sites without re-listing them (a closure would
        // have to mutably re-borrow them per batch element).
        macro_rules! process_relay_event {
            ($event:expr) => {
                relay_event_guard::process_relay_event(
                    $event,
                    &mut kernel,
                    &config.relay_text_interceptors,
                    &config.relay_connected_hooks,
                    &command_tx_self,
                    &mut relay_controls,
                    &mut slot_to_url,
                    &pool,
                    &mut next_relay_generation,
                    &mut connected_relays,
                    &mut connected_urls,
                    &update_tx,
                    &mut last_emit,
                    &mut startup_sent,
                    running,
                )
            };
        }

        // #1264: serve a BOUNDED batch of staged backlog events this iteration
        // (up to RELAY_BACKLOG_DRAIN_BATCH) so the backlog drains faster than a
        // sustained relay flood fills it — then ALWAYS fall through to the
        // single blocking `recv_timeout` below. A non-empty backlog therefore no
        // longer bypasses the one wait per iteration (D8), which kills the
        // busy-spin that previously pinned the CPU under flood.
        for event in scheduler.drain_backlog_batch() {
            process_relay_event!(event);
        }

        // ── Relay event lane ─────────────────────────────────────────────
        // Block up to compute_wait so emit-hz is respected without busy-spin.
        // This `recv_timeout` is the loop's SINGLE blocking point (D8). A
        // command received during the wait is replayed as `first_command` so
        // the next iteration dispatches it on the priority lane (no added
        // latency).
        //
        // #1264: when backlog work remains (the batch did not exhaust it) we
        // pass a ZERO wait so the loop keeps draining promptly — but we STILL
        // call `recv_timeout`, so the single blocking point is reached every
        // iteration (no busy-spin / no D8 violation: a zero-timeout `recv` is
        // the one wait, it simply returns immediately when nothing is queued).
        //
        // Phase F: the inbound item is `PoolEvent` (push-model). Stale-event
        // filtering moved into `handle_relay_event` itself — the helper
        // resolves `RelayHandle.slot()` → `(url, role)` via the `slot_to_url`
        // side-map and the `relay_controls` entry, dropping any handle whose
        // generation no longer matches the slot's current generation. The
        // pool's translator already drops events with a stale slot-generation,
        // so this is belt-and-braces.
        let wait = if scheduler.has_backlog() {
            std::time::Duration::ZERO
        } else {
            command_drain.relay_wait(compute_wait(&kernel, running, last_emit, emit_hz))
        };
        match scheduler.next_after_drain(&inbox, wait) {
            LoopStep::Command(command) => {
                // Woken by a command during the blocking wait — replay it on
                // next iteration's priority lane (zero added latency).
                first_command = Some(command);
            }
            LoopStep::Shutdown => {
                close_relays(
                    &mut relay_controls,
                    &mut slot_to_url,
                    &pool,
                    &mut connected_relays,
                    &mut kernel,
                );
                connected_urls.clear();
                return;
            }
            LoopStep::Idle => {
                // Timeout (normal idle tick) — fall through to idle work.
            }
            LoopStep::Relay(event) => {
                process_relay_event!(event);
            }
        }

        // ── Idle work (runs on every iteration after relay poll) ─────────
        // Flush any time-gated view requests (e.g. contacts_deadline) and
        // run the M2 planner tick only while the actor is running. Before
        // Start these would spawn relay workers (via send_all_outbound) and
        // trigger relay-lifecycle events that emit spurious snapshots on the
        // update channel even though no consumer is listening — the root
        // cause of the S2 retention leak (T114b / s2-retention-audit.md).
        // The publish engine tick below already carries the same running gate
        // for the same reason. Pending profile claims, deferred view
        // requests, and lifecycle triggers all survive in kernel state until
        // Start flushes them through spawn_missing_relays + the first
        // running-gated idle tick.

        // V-64: drive wall-clock-gated sweeps (e.g. NIP-47 pending-payment
        // TTL expiry) even when no relay frame arrives. The interceptor's
        // default `on_idle_tick` is a no-op; the nmp-nip47 impl uses this
        // hook to close expired pay_invoice correlations via
        // `record_action_failure`. No running gate — sweeps must fire even
        // before Start so that entries enqueued during connection setup are
        // not orphaned if the relay never connects.
        {
            for interceptor in &config.relay_text_interceptors {
                let extra = interceptor.on_idle_tick(&mut kernel);
                if !extra.is_empty() {
                    send_all_outbound(
                        &mut relay_controls,
                        &mut slot_to_url,
                        &pool,
                        &mut kernel,
                        &mut next_relay_generation,
                        extra,
                    );
                }
            }
        }

        if running {
            let pending = kernel.pending_view_requests();
            if !pending.is_empty() {
                send_all_outbound(
                    &mut relay_controls,
                    &mut slot_to_url,
                    &pool,
                    &mut kernel,
                    &mut next_relay_generation,
                    pending,
                );
            }
        }
        // T142 — M2 planner tick: drain the subscription lifecycle's trigger
        // inbox. Per D8, an empty inbox is a zero-cost no-op (single
        // `is_empty()` check — no allocation, no compile pass). When
        // triggers are queued (e.g. FollowListChanged A11, Nip65Arrived A1)
        // this produces REQ/CLOSE WireFrames that are converted to
        // OutboundMessages and sent to the relay pool. Placed after M1
        // `pending_view_requests()` to ensure M1 CLOSE frames are enqueued
        // before M2 opens new subs (spec §3.1 placement rationale).
        if running {
            let wire_frames = kernel.drain_lifecycle_tick();
            if !wire_frames.is_empty() {
                let outbound = wire_frames_to_outbound(wire_frames, &mut kernel);
                send_all_outbound(
                    &mut relay_controls,
                    &mut slot_to_url,
                    &pool,
                    &mut kernel,
                    &mut next_relay_generation,
                    outbound,
                );
            }
        }
        // W6 — claim-expansion idle tick: advance the per-claim Phase 1/2/3
        // state machine once per actor idle iteration. Per D8, an empty
        // `pending_claims` map is a zero-cost no-op (single `is_empty()` check
        // in `poll_claim_expansion`, no allocation, no iteration). When claims
        // are pending, the state machine applies budget checks and promotes
        // Phase-1 claims to Phase 2 by enqueuing a `CompileTrigger::ViewOpened`
        // via `advance_to_phase2`; the resulting REQ frames surface on the NEXT
        // iteration's `drain_lifecycle_tick` call above. Per D4, this is the
        // sole writer of `pending_claims` — actor single-writer invariant.
        // `poll_claim_expansion` always returns `Vec::new()` today (W5 contract);
        // the `if !msgs.is_empty()` guard is forward-compatible with W7+ where
        // the controller may route fallback REQs as direct OutboundMessages.
        if running {
            let expansion_msgs = kernel.poll_claim_expansion(Instant::now());
            if !expansion_msgs.is_empty() {
                send_all_outbound(
                    &mut relay_controls,
                    &mut slot_to_url,
                    &pool,
                    &mut kernel,
                    &mut next_relay_generation,
                    expansion_msgs,
                );
            }
        }
        kernel.flush_relay_scores_if_dirty();
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
                    &mut slot_to_url,
                    &pool,
                    &mut kernel,
                    &mut next_relay_generation,
                    retry_frames,
                );
            }
        }
        if running {
            sweep_temporary_idle_relays(
                &mut relay_controls,
                &mut slot_to_url,
                &mut connected_urls,
                &pool,
                &mut kernel,
                Instant::now(),
                TEMPORARY_RELAY_IDLE_GRACE,
            );
        }
        // #1069 — bounded GC pass on the actor idle tick (audit Finding 1:
        // `gc_step` was never called in production, so on-device store growth,
        // NIP-40 expiry, and LRU eviction were all dead). Mirrors the T127
        // publish tick above: piggy-backs the existing ≤250 ms `compute_wait`
        // loop wake with a wall-clock gate so it fires at most once per
        // `GC_TICK_INTERVAL` (60 s, `gc.md` §3) — no new sleep loop, no timer
        // thread (D8 / "no polling"). When the gate has not elapsed this is a
        // single `Instant::elapsed()` compare — heap-free, no false wakeups.
        //
        // `Kernel::run_gc_step` derives `now_secs` from the injected kernel
        // clock (D7/D9 — deterministic under replay/`FixedClock`); the store's
        // own `gc.rs` budget loops bound the worst-case latency to ~50 ms so the
        // mailbox is never blocked (`gc.md` §3, §8).
        if running && last_gc.elapsed() >= GC_TICK_INTERVAL {
            kernel.run_gc_step();
            last_gc = Instant::now();
        }
        // ADR-0045 §5 — chunked continuation for store-cache serves. Drains
        // ONE aggregate per-tick budget chunk (`cache_serve_tick_budget`,
        // 2× the visible window) across ALL pending serves, resuming
        // partially-completed interests via their per-query cursor. Like the
        // gc tick above this piggybacks the existing ≤250 ms `compute_wait`
        // wake — no new sleep loop, no timer thread (D8 / "no polling").
        // An empty queue costs one bool check. Runs BEFORE the `flush_due`
        // emit below, so served events land in this tick's snapshot (D1).
        if running && (kernel.has_pending_cache_serves() || kernel.has_cache_serve_wakeups()) {
            kernel.run_cache_serve_step();
        }
        // ── V-06 / #960: drain kernel-emitted NIP-42 AUTH signs ──────────
        // `handle_message` enqueues an AUTH kind:22242 for any relay lane whose
        // active account is a REMOTE signer; route each through the async signer
        // port (park under the `Auth` sink) — see `auth_sign::drain_pending_auth_signs`.
        auth_sign::drain_pending_auth_signs(
            &mut kernel,
            &identity,
            &mut parked_ops,
            &mut auth_sign::RouteCtx {
                running,
                queued_publish_outbound: &mut queued_publish_outbound,
                relay_controls: &mut relay_controls,
                slot_to_url: &mut slot_to_url,
                pool: &pool,
                next_relay_generation: &mut next_relay_generation,
            },
        );
        // ── Poll the unified parked-op queue (ADR-0050 §D2) ──────────────
        // ONE `retain_mut` over ONE `Vec<ParkedOp>` replaces the two former
        // drains (the inline publish block + `resolve_pending_sign_return`). Each
        // op is polled once per tick (D8 — `SignerOp::poll` is non-blocking; the
        // deadline is the wall-clock gate). Projection / continuation sinks
        // resolve against the kernel in `resolve_parked_op`; the `Publish` sink
        // hands back a `PublishObligation` (the loop owns relay routing).
        // Obligations are collected during the drive and run after it so the
        // drain's `&mut kernel` borrow never overlaps `route_dispatch_outbound`.
        // Empty `parked_ops` is a heap-free zero-item drive.
        //
        // `ParkedSignerOps::drive` is the ONE canonical drain driver (#1753),
        // shared verbatim with the wasm `KernelReducer`; the native loop drives
        // it on the idle tick, the wasm reducer drives it on a sign-completion
        // message — same `retain_mut`, no parallel copy.
        if !parked_ops.is_empty() {
            let pending_sign::DrainBatch {
                publish: publish_obligations,
                auth: auth_obligations,
                changed: any_changed,
            } = parked_ops.drive(&mut kernel);
            // V-06 / #960: execute the NIP-42 AUTH obligations the `Auth` sink
            // handed back (re-enter `dispatch_signed_auth` / `fail_auth_sign` and
            // route outbound) — see `auth_sign::run_auth_obligations`. Runs here
            // after the retain so the drain's `&mut kernel` borrow has ended.
            auth_sign::run_auth_obligations(
                &mut kernel,
                auth_obligations,
                &mut auth_sign::RouteCtx {
                    running,
                    queued_publish_outbound: &mut queued_publish_outbound,
                    relay_controls: &mut relay_controls,
                    slot_to_url: &mut slot_to_url,
                    pool: &pool,
                    next_relay_generation: &mut next_relay_generation,
                },
            );
            // Execute the publish obligations the `Publish` sink handed back,
            // preserving ALL prior terminal behaviours exactly: a resolved sign
            // routes via the parked `target` + `correlation_id_override`; a
            // failure / timeout surfaces the toast and (for a dispatched action)
            // records the `"failed"` verdict so the host spinner clears (D6).
            for obligation in publish_obligations {
                match obligation {
                    PublishObligation::Publish {
                        signed,
                        p_tags,
                        target,
                        correlation_id_override,
                    } => {
                        let outbound = kernel.publish_signed_to_with_correlation(
                            &signed,
                            &p_tags,
                            target,
                            correlation_id_override,
                        );
                        route_dispatch_outbound(
                            running,
                            &mut queued_publish_outbound,
                            &mut relay_controls,
                            &mut slot_to_url,
                            &pool,
                            &mut kernel,
                            &mut next_relay_generation,
                            outbound,
                        );
                    }
                    PublishObligation::Failed {
                        toast,
                        correlation_id_override,
                        reason_code,
                    } => {
                        kernel.set_last_error_toast(Some(toast.clone()));
                        // Recorded BEFORE `emit_now` (below) so this tick's
                        // snapshot drains it; `None` (a `react` / `follow` park)
                        // is a no-op — nothing is waiting on an id. A
                        // capability/signer denial carries the curated
                        // `reason_code` (S7, #1754) so the host localizes the
                        // failure; an un-coded failure stays prose-only.
                        if let Some(id) = correlation_id_override {
                            kernel.record_action_failure_coded(id, toast, reason_code, None);
                        }
                    }
                }
            }
            // Surface the changes immediately rather than waiting up to one
            // periodic flush tick — matches the prior per-op `emit_now`.
            if any_changed && running {
                emit_now(&mut kernel, running, &update_tx, &mut last_emit);
            }
        }
        // Only emit when state actually changed; do not emit on every
        // idle tick (D8: zero false-wakeup allocations after warmup).
        if flush_due(&kernel, running, last_emit, emit_hz) {
            emit_now(&mut kernel, running, &update_tx, &mut last_emit);
        }
    }
}
