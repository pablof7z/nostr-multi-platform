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
#[cfg(feature = "native")] pub(crate) mod typed_projections;
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
#[cfg(feature = "native")] mod auth_sign;
#[cfg(feature = "native")] mod builtin_projections;
#[cfg(feature = "native")] mod capability_worker;
#[cfg(feature = "native")] mod compat;
#[cfg(feature = "native")] mod config;
#[cfg(feature = "native")] mod dispatch;
// ADR-0050 §D1/§D3b signer-port dispatch helpers (cipher verbs + completion
// delivery), split out to keep `dispatch.rs` within budget. Native-only (uses
// the native `ActorContext`).
#[cfg(feature = "native")] mod fairness;
#[cfg(feature = "native")] mod signer_port_dispatch;
mod signer_source;
// ADR-0050 §D3a — the single waking actor inbox. `ActorMail` + `CommandSender`
// are always-compiled (the always-compiled `substrate::protocol` seam hands
// `CommandSender` to workers, and `ActorCommand` itself is always-compiled);
// the relay-side scheduler / sink / `Inbox` are `native`-gated inside.
mod inbox;
// Inbox command/relay lane priority + fairness tests, extracted from `inbox.rs`
// to keep that file under the 500 LOC hard cap (AGENTS.md).
#[cfg(all(test, feature = "native"))] mod inbox_lane_tests;
// Always-compiled port continuations (named by the always-compiled
// `ActorCommand` sign / cipher verbs; not `native`-gated).
mod continuations;
// Generic raw signed-event forwarding dispatch. Native-only: depends on
// `nmp_network::pool::Pool` for outbound `["EVENT", ...]` frames. Policy
// crates provide target selection through a substrate trait object.
#[cfg(all(test, feature = "native"))] mod app_managed_signer_tests;
#[cfg(all(test, feature = "native"))] mod cipher_for_account_tests;
#[cfg(all(test, feature = "native"))] mod nip42_async_auth_tests;
#[cfg(feature = "native")] mod outbound;
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
#[cfg(all(test, feature = "native"))] mod protocol_panic_isolation_tests;
#[cfg(all(test, feature = "native"))] mod publish_relay_dispatch_tests;
#[cfg(feature = "native")] pub(crate) mod raw_event_forwarder;
// Per-URL relay-worker state (RelayControl, RelayConnectionKind).
// Extracted from mod.rs to keep it within the 500-LOC ceiling (AGENTS.md).
#[cfg(feature = "native")] mod relay_control;
#[cfg(feature = "native")] pub(in crate::actor) use relay_control::{RelayControl, RelayConnectionKind};
#[cfg(feature = "native")] mod relay_event_guard;
#[cfg(feature = "native")] mod relay_idle;
#[cfg(feature = "native")] mod relay_mgmt;
#[cfg(feature = "native")] mod relay_reconnect;
mod relay_roles;
#[cfg(all(test, feature = "native"))] mod relay_url_canonical_tests;
#[cfg(all(test, feature = "native"))] mod send_gate_universal_tests;
#[cfg(feature = "native")] mod session_persistence;
#[cfg(all(test, feature = "native"))] mod session_persistence_tests;
#[cfg(all(test, feature = "native"))] mod sign_event_for_account_tests;
#[cfg(all(test, feature = "native"))] mod signer_port_test_harness;
#[cfg(all(test, feature = "native"))] mod tests;
#[cfg(feature = "native")] mod tick;
#[cfg(all(test, feature = "native"))] mod v87_d1_startup_tests;
#[cfg(all(test, feature = "native"))] mod v90_capability_worker_tests;
// `ActorCommand` and sub-enums — always-compiled command payload types.
// Extracted to keep this file within the 500-LOC ceiling (AGENTS.md).
mod actor_command;
pub use actor_command::{
    ActionLedgerCommand, ActorCommand, ContactsCommand, IdentityCommand, InterestsCommand,
    LifecycleCommand, PublishCommand, RelayCommand, RefsCommand, SignCommand, SignerSource,
};
#[cfg(any(test, feature = "test-support"))]
pub use actor_command::TestSupportCommand;
// Native actor entry point — `run_actor_with_observers`.
// Extracted to keep this file within the 500-LOC ceiling (AGENTS.md).
// `actor_loop` contains the extracted main event loop body called by `actor_run`.
#[cfg(feature = "native")] mod actor_loop;
#[cfg(feature = "native")] mod actor_run;
#[cfg(feature = "native")] pub use actor_run::run_actor_with_observers;

// V-01 Phase 1c: capability callback and identity runtime are native actor runtime only.
#[cfg(feature = "native")] use crate::capability_socket::new_capability_callback_slot;
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
#[cfg(feature = "native")] pub use commands::{register_c_observer, LifecycleObserverRegistration};
// D0: NIP-46 remote signing is an app noun — the bunker-handshake slot is
// re-exported so the `ffi` module can build it, hand one clone to the actor's
// `IdentityRuntime`, and capture the other in the built-in
// `"bunker_handshake"` typed snapshot-projection closure.
// V-01 Phase 1c: bunker types are native actor / FFI only.
#[cfg(feature = "native")] pub(crate) use commands::BunkerHandshakeSlot;
// `nmp-ffi`'s `nmp_app_new` constructs the bunker-handshake slot before
// handing it to the actor; promoted to `pub` for the extracted crate.
#[cfg(feature = "native")] pub use commands::new_bunker_handshake_slot;
// ADR-0048 D6: generalised remote-signer health slot (hard-break rename of
// the former `BunkerConnectionStateSlot` — no compat aliases). The DTO itself
// stays `commands`-private; callers drive it only through the actor commands.
#[cfg(feature = "native")] pub use commands::{new_signer_state_slot, SignerStateSlot};
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
#[cfg(feature = "native")] pub use commands::KernelEventObserverRegistration;
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
// V-01 Phase 1c: public actor config types (native actor runtime only).
#[cfg(feature = "native")]
pub use config::{ActorChannels, ActorConfig, ActorConfigSources, ActorRuntimeSlots};

// ADR-0050 §D3a — always-compiled inbox transport types. `CommandSender` is the
// single command-send seam handed to host code, protocol/capability workers,
// the broker adapter, and the actor's self-feedback path; `ActorMail` is what
// the unified inbox carries. Both name no protocol concept (D0).
pub use inbox::{ActorMail, CommandSendError, CommandSender};
// ADR-0050 §D1 — always-compiled port continuations named by the (always-
// compiled) `ActorCommand` sign / cipher verbs.
pub use continuations::{CipherContinuation, SignContinuation};

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
pub(crate) const GC_TICK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

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
#[cfg(feature = "native")] pub use relay_roles::nostrconnect_relay_url;

// ─────────────────────────────────────────────────────────────────────────────
// V-01 Phase 1c: the actor runtime — per-URL relay handles, the public
// entry points (`run_actor*`), and every loop / dispatch helper below —
// sits on top of the native `relay_worker`. Gated behind `native` so the
// crate compiles without the WebSocket transport. Everything above (the
// `ActorCommand` enum, observer types, `relay_roles`) stays always-compiled.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(all(feature = "native", any(test, feature = "test-support")))]
pub use compat::run_actor;
#[cfg(feature = "native")]
#[allow(unused_imports)]
pub use compat::run_actor_with_lifecycle_observer;
