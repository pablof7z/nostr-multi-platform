mod actor;
mod app;
pub mod bunker_hook;
pub mod external_signer_hook;

// SHARED FlatBuffers `ProfileCard` row type, mounted at the crate root so the
// profile-cluster generated bindings can resolve it.
//
// `profile.fbs` / `claimed_profiles.fbs` / `resolved_profiles.fbs` all `include
// "profile_card.fbs"` and reference its `ProfileCard` table. `flatc` (no
// `--gen-all`) emits `ProfileCard` ONLY into `profile_card_generated.rs` and
// drops a crate-root `use crate::profile_card_generated::*;` into each per-key
// `*_generated.rs`. That glob only sees items at the *top* of
// `profile_card_generated`, but the generated leaf types are nested under
// `nmp::kernel`. So this wrapper hides the generated `pub mod nmp` inside
// `inner` and flat-re-exports the `nmp::kernel` leaf types at the module root —
// the per-key generated files' glob then resolves `ProfileCard` /
// `ProfileCardArgs` by short name. Mirrors the `op_feed.fbs` →
// `timeline_snapshot.fbs` include precedent in `crates/nmp-nip01/src/lib.rs`.
#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
pub(crate) mod profile_card_generated {
    mod inner {
        #![allow(
            clippy::all,
            dead_code,
            deprecated,
            missing_docs,
            non_camel_case_types,
            non_snake_case,
            unsafe_code,
            unused_imports
        )]
        include!("kernel/typed_projections/generated/profile_card_generated.rs");
    }
    pub use inner::nmp::kernel::*;
}

// V-112 (ADR-0042): the shared FlatBuffers `TimelineItem` row cluster
// (`timeline_item.fbs`, `timeline_item_generated.rs`, and the
// `timeline_item_generated` wrapper mod that mirrored
// `profile_card_generated` above) was deleted — its only consumers were the
// retired `author_view.fbs` / `thread_view.fbs` typed projections.

// V6 Stage 1 — Swift `Decodable` emitter input surface. Feature-gated:
// `cargo run -p nmp-core --features codegen-schema --bin dump_projection_schemas`
// dumps one JSON schema per pilot projection type for `nmp-codegen gen swift`
// to consume. Off by default — shipped artifacts never link `schemars`.
#[cfg(feature = "codegen-schema")]
pub mod codegen_schema;
// Promoted from `mod capability_socket` so `nmp-ffi` can reach
// `dispatch_capability` / `new_capability_callback_slot` /
// `CapabilityCallbackSlot` through `nmp_core::__ffi_internal::*`. The
// socket is the substrate of the capability-callback seam; nothing in it
// names an app or protocol noun.
#[doc(hidden)]
pub mod capability_socket;
// V-33: shared display-string helpers (bech32 abbreviation, avatar tint
// djb2, relative-time bucketing) — canonical home for the cross-surface
// formatting primitives every NIP crate / kernel module / host-app
// projection previously duplicated.
pub mod display;
// Step 11 final — the C-ABI surface that used to live in `mod ffi;` now lives
// in the standalone `nmp-ffi` crate (`docs/architecture/crate-boundaries.md`
// §5 step 11-final). The substrate types the FFI marshals are re-exported
// through the public surface below + the `__ffi_internal` module so the
// extracted crate can name them through normal Rust paths.
//
// `mod ffi;` is gone — `pub use ffi::*` at the bottom of this file is gone
// too — consumers reach the symbols through `nmp_ffi::*` directly.
// ffi_guard: pure catch_unwind wrapper. Not I/O-bound; kept always-on
// because actor/commands/* use it on the native side (also actor is always
// compiled until Phase 1c decoupling). Promoted from `mod ffi_guard` to
// `pub mod ffi_guard` so the extracted `nmp-ffi` crate can reach
// `guard_ffi_callback` through a normal Rust path. The guard is substrate-
// grade (no app or protocol nouns); making it public is a layer-shape
// concession, not a noun leak.
#[doc(hidden)]
pub mod ffi_guard;
// Step 8 phase A — the keepalive FSM moved with the relay worker to
// `nmp-network::keepalive`. It's purely transport-internal; `nmp-core`
// no longer re-exports it.
mod kernel;
mod kernel_action;
mod kernel_reducer;
/// V-57 P2 — canonical Nostr kind constants for the entire workspace.
/// Single source of truth for the integer kind numbers used on the wire.
/// See [`kinds`] for the migration rationale.
pub mod kinds;
pub mod nip19;
pub mod nip21;
// Subscription compiler — internal path for nmp-core consumers.
// External callers must depend on `nmp-planner` directly and use
// `nmp_planner::*`; the `nmp_core::planner` re-export path is deleted
// (#1608, D0/D3: facades leak planner internals into the app-facing surface).
// Only items actively used by nmp-core internals are re-exported here; the
// old catch-all list is trimmed so unused-import warnings become impossible.
pub(crate) mod planner {
    pub use nmp_planner::compiler::{MailboxCache, MailboxSnapshot, SubscriptionCompiler};
    pub use nmp_planner::interest::{
        bounded_search_query, HintSource, InterestId, InterestLifecycle, InterestScope,
        InterestShape, LogicalInterest, NaddrCoord, Pubkey, RelayHint, RelayUrl,
    };
    // Test-only: `InMemoryMailboxCache` and `PTagRouting` are only referenced
    // in `#[cfg(test)]` modules inside nmp-core; gate them so the production
    // build stays warning-free under `-D warnings`.
    #[cfg(test)]
    pub use nmp_planner::compiler::InMemoryMailboxCache;
    #[cfg(test)]
    pub use nmp_planner::interest::PTagRouting;
    pub use nmp_planner::plan::{canonical_filter_hash, CompiledPlan, PlannerError, RelayAttribution, SubShape};
    // W4 — warm-relay score lookup seam + lookup-aware selection.
    pub use nmp_planner::selection::apply_selection_with_lookup;
    // Internal call sites that reach into `interest::EventId` and similar
    // sub-module items that aren't re-exported at the planner crate root.
    pub use nmp_planner::interest;
}
/// V-52 — single-relay browsing via the `nmp.browse_relay` action namespace.
///
/// Manual relay-bypass path (D3 explicit opt-out): builds a
/// [`nmp_planner::interest::LogicalInterest`] with `relay_pin = Some(url)` and
/// dispatches `ActorCommand::PushInterest`. This is not a standard app path —
/// the host must explicitly register [`browse::BrowseRelayModule`] and the
/// caller must supply a validated relay URL. NIP-65 fan-out is suppressed.
pub mod browse;
pub mod publish;
mod relay;
mod transport;
// ADR-0064 / S2 (#1750) — the open write-command byte transport: decode +
// fail-closed gates + the opaque-payload carry. Public so the wasm runtime and
// the native FFI byte doorway both reach the one inbound decode path.
pub use transport::dispatch_envelope;
// Step 8 phase A — `relay_protocol` and `relay_worker` moved to
// `nmp-network`. They are re-imported here only through the (gated) actor
// runtime path; the public re-exports below preserve the prior
// `nmp_core::relay_protocol::*` surface (no-op for downstream crates that
// imported through the old path — they should migrate to `nmp_network`).
//
// V-38: the `wallet` module is gone — the NIP-47 wallet runtime + the
// `nmp.wallet.pay_invoice` `ActionModule` moved to `crates/nmp-nip47`. The
// kernel no longer depends on `nmp-nwc`, and `nmp-core` no longer has a
// `wallet` Cargo feature. See `docs/architecture/crate-boundaries.md`
// §5 step 7 for the migration brief.
pub mod remote_signer;
// Deterministic 64-bit hash helper — internal path for nmp-core.
// External callers must depend on `nmp-planner` directly and use
// `nmp_planner::stable_hash::stable_hash64` (#1608, compat facade deleted).
pub(crate) mod stable_hash {
    pub use nmp_planner::stable_hash::*;
}
// Event-storage abstraction — internal path for nmp-core.
// External callers must depend on `nmp-store` directly and use
// `nmp_store::*` (#1608, compat facade deleted).
pub(crate) mod store {
    pub use nmp_store::*;
}
pub mod projection_emission; // ADR-0055 R6-S2: byte-equality typed-projection omit helper.
// Step 11 final — shared substrate slot aliases the FFI shell (`nmp-ffi`) and the
// actor runtime (`crate::actor`) both reach into. Used to live in `crate::ffi::mod.rs`
// (private); promoted here so the crate-private actor module can still name them after
// the FFI extraction. `pub` because nmp-ffi reaches them through `nmp_core::slots::*`.
pub mod signer_catalog;
pub mod slots;
pub mod subs;
pub mod substrate;
pub mod tags;
// Target-conditional time shim: `web_time` on wasm32, `std::time` on native.
// Wasm-reachable kernel code imports `Instant`, `SystemTime`, `UNIX_EPOCH`
// from here so `performance.now()` / `Date.now()` back them on wasm32
// (where the `std` implementations abort). See `time.rs` for rationale.
pub mod time;
pub mod ui_token;
mod update_envelope;
pub mod util;

pub use app::{
    resolve_open_uri, KernelAction, KernelUpdate, KernelViewSpec, OpenUriError, OpenUriRouting,
    VIEW_ADDRESSABLE, VIEW_PROFILE, VIEW_THREAD,
};
pub use bunker_hook::{install_bunker_hook, new_bunker_hook_slot, BunkerHookFn, BunkerHookRequest, BunkerHookSlot};
pub use external_signer_hook::{install_external_signer_hook, new_external_signer_hook_slot, ExternalSignerHookFn, ExternalSignerHookRequest, ExternalSignerHookSlot};
// Step 11 final — `NmpApp` opaque handle + the `nmp_app_*` symbol family
// moved to the standalone `nmp-ffi` crate (`nmp_ffi::NmpApp`). `nmp-core`
// no longer exposes `ffi::*` at all.
pub use kernel::{
    read_eligible_relay_urls, AppRelay, AppRelayList, AppRelaySlot, Kernel, ProfileLiveness,
    KERNEL_BUILTIN_PROJECTION_KEYS,
};
pub use kernel::pull::{pull_page_over, PullError, PullLimits, PullScope}; // ADR-0058
pub use kernel::pull_cursor::{PullCursorId, PullCursorMode};
pub use kernel::pull_wake::{decode_pull_wake_batch, PullWakeRow, PULL_WAKE_KEY};
// ADR-0049 — the composition ledger (explain-the-composition surface) and its
// record types. Re-exported at the crate root so `nmp-ffi` (the C-ABI host) and
// downstream composition crates can name them without reaching into `kernel`.
pub use kernel::{
    CompositionLedger, CompositionRecord, Disposition, COMPOSITION_REPORT_SCHEMA_VERSION,
};

// Injectable kernel wall-clock trait. Re-exported (always) so the `pub`
// `slots::KernelClockSlot` alias (`Arc<Mutex<Option<Arc<dyn Clock>>>>`) is
// nameable across crates. Production installs nothing (the kernel keeps its
// `SystemClock`); only the test-support `MonotonicSecondClock` is constructible
// downstream.
pub use kernel::Clock;
// Test-support: advanceable kernel clock external e2e tests install through the
// FFI `NmpApp::set_kernel_clock_for_test` seam to stamp strictly-increasing
// `created_at` deterministically (no wall-clock sleep — D8).
#[cfg(any(test, feature = "test-support"))]
pub use kernel::MonotonicSecondClock;
// W2 — relay-author-score types. Re-exported so nmp-testing integration tests
// and downstream crates (W4, W5) can access `ClaimOutcome`, `RelayAuthorScore`,
// and `RelayAuthorScoreMap` without reaching into the private `kernel` module.
pub mod relay_score {
    pub use super::kernel::relay_score::{
        ClaimOutcome, RelayAuthorScore, RelayAuthorScoreMap, DECAY_HALFLIFE_DAYS,
        MAX_EXPANSION_CONCURRENCY, MAX_RELAYS_TRIED_PER_CLAIM, PER_CLAIM_TOTAL_BUDGET_MS,
        PER_RELAY_REQ_TIMEOUT_MS, PHASE_1_BUDGET_MS, WARM_THRESHOLD,
    };
}
// V-38: NIP crates (`nmp-nip47`) registering per-lane NIP-42 signers need the
// `AuthSignerFn` alias for their `Kernel::set_relay_auth_signer(...)` call.
// Substrate-grade (D0): no protocol nouns — generic Schnorr signer callback.
pub use kernel::{wallet_access::KernelWalletAccess, AuthSignerFn}; // KernelWalletAccess: ADR-0052 §D5 wallet/zap adapter
// V-51 phase 4 (validation harness) — the projection's three public types
// reachable from `nmp-testing` and the chirp-repl. `RoutingTraceProjection`
// is the bounded ring-buffer the kernel hands to production composition
// (via `routing_trace()` → `set_routing_substrate` factory →
// `GenericOutboxRouter::with_trace_observer`); `PublishTraceEntry` /
// `SubscriptionTraceEntry` are the entry shapes the `snapshot_*` accessors
// return. See `kernel::routing_trace` module doc.
pub use kernel::routing_trace::{
    PublishTraceEntry, RoutingTraceProjection, SubscriptionTraceEntry,
    DEFAULT_ROUTING_TRACE_CAPACITY,
};
// V-51 phase 2 — JSON DTO renderer. Consumer-side helper: turns a
// projection snapshot into a Swift/wasm-friendly JSON value the FFI symbol
// (`nmp_app_recent_routing_decisions`) and the wasm runtime
// (`recent_routing_decisions`) both ship to their respective hosts.
pub use kernel::routing_trace_dto::{projection_to_json, ROUTING_TRACE_SCHEMA_VERSION};
// V-01 Stage 3 — the wire-transport-agnostic frame enum the kernel ingests.
// Promoted to the public surface so the wasm32 `BrowserRelayDriver` (lives
// in `nmp-network::browser_driver` as of step 8 phase C) can be bridged from
// `web_sys::MessageEvent` / `CloseEvent` through the
// `nmp-wasm::relay_pool::build_handlers` callback bag.
// Substrate-grade (D0): no app/protocol nouns.
pub use kernel::RelayFrame;
pub use kernel_reducer::KernelReducer;
pub use relay::canonical_relay_url;
// V-01 Stage 3 — the per-frame outbound type (`role`, `relay_url`, `text`) the
// kernel produces and any transport (native `relay_worker`, wasm
// `BrowserRelayDriver` — both in `nmp-network` as of step 8 phase C) consumes.
// Fields stay `pub(crate)` so the kernel remains the single writer; external
// callers read via accessors.
pub use relay::{OutboundMessage, RelayRole};
pub use remote_signer::RemoteSignerHandle;
pub use update_envelope::{
    decode_snapshot_envelope, decode_snapshot_typed_projections, decode_update_frame, encode_panic,
    encode_snapshot_frame, panic_message, PanicFrame, RelayStatusEntry, SnapshotEnvelope,
    TypedProjectionData, UpdateEnvelope, UpdateFrameBytes, UpdateFrameDecodeError,
    WireProjectionState, WireSubscriptionEntry, SNAPSHOT_SCHEMA_VERSION,
};

/// Public decode surface for the kernel-owned (Tier-2) typed-projection
/// sidecar (ADR-0037).
///
/// Pair these per-key decoders with [`decode_snapshot_typed_projections`],
/// which returns the snapshot's [`TypedProjectionData`] entries: look an entry
/// up by `key` (e.g. [`typed_projections::PUBLISH_QUEUE_SCHEMA_ID`]) and pass
/// its `payload` to the matching `decode_*` function to get a typed Rust
/// struct. The Tier-3 envelope fields (rev/running/metrics/relay status)
/// travel separately — read them via [`decode_snapshot_envelope`].
///
/// The module is the documented extension point for the Tier-2 cluster (one
/// `pub use` line per key).
pub mod typed_projections {
    pub use crate::kernel::public_typed_projections::*;
}

// Stage 4 of NIP-46 wiring: app/FFI composition translates app-neutral
// broker events into actor commands. The `actor` module is crate-private so
// this re-export is the only Rust-side path for adapters that need to push
// `AddSigner` / `BunkerHandshakeProgress` back to the actor. The enum
// variants themselves are already `pub`.
//
// `SignerSource` is re-exported alongside so the FFI sign-in shims and the
// broker adapter can name `SignerSource::{LocalNsec, BunkerUri, RemoteHandle}`
// when constructing an `AddSigner` command.
//
// `SignContinuation` is the boxed sign-outcome callback carried by the
// `ActorCommand::SignEventForAccount` port (ADR-0043 Decision 2). Re-exported
// so protocol crates that consume the port through
// `ProtocolCommandContext::sign_event_for_account` (e.g. `nmp-nip57`'s zap
// command) can name it — chiefly in tests that drive the continuation directly.
pub use actor::{ActorCommand, CipherContinuation, SignContinuation, SignerSource};
// ADR-0050 §D3a — the unified actor-inbox transport seam. `CommandSender` is
// the single command-send handle passed to relay-connected hooks, DM inbox
// chains, and similar substrate seams that post commands from worker threads.
// `CommandSendError` is `send`'s error (mpsc-`SendError` parity).
// `ActorMail` is the raw inbox discriminant — test-support only (#1608: not
// part of the stable public API; external code that needs a test channel
// should use `nmp_core::testing::spawn_actor()` instead).
pub use actor::{CommandSendError, CommandSender};
#[cfg(any(test, feature = "test-support"))]
pub use actor::ActorMail;

// Step 11 final — every `nmp_app_*` `extern "C"` symbol that used to be
// re-exported from `ffi::` now lives in the standalone `nmp-ffi` crate.
// Consumers that previously named the symbols through `nmp_core::` should
// migrate to `nmp_ffi::*`. The `NmpApp` opaque handle moved with the
// symbols. See `docs/architecture/crate-boundaries.md` §5 step 11-final.
//
// V-38: the `nmp_app_wallet_*` FFI symbols moved to `nmp-ffi::wallet` as
// thin shims routing through `nmp.wallet.{connect,disconnect,pay_invoice}`
// (dispatch_action). The actual wallet runtime lives in `crates/nmp-nip47`.

// T118 / G3 — lifecycle observer wire-shape exposed for integration tests
// (the `LifecycleObserverFn` is a plain `extern "C" fn` shape) and the
// phase-code constants the observer must interpret. The actor module is
// crate-private, so this is the only Rust-side surface for the wire shape.
#[cfg(any(test, feature = "test-support"))]
pub use actor::{LifecycleObserverFn, LIFECYCLE_PHASE_BACKGROUND, LIFECYCLE_PHASE_FOREGROUND};

// T146 — kernel event observer surface exposed to per-app Rust crates
// (`nmp-app-chirp`, future app-specific crates, ...). Apps register typed
// `Arc<dyn KernelEventObserver>`s via [`NmpApp::register_event_observer`].
// The FFI shape (`KernelEventObserverFn` etc.) is the C-ABI channel
// Swift / Kotlin bridges use directly through
// `nmp_app_register_event_observer`.
pub use actor::{KernelEventObserver, KernelEventObserverFn, KernelEventObserverId};

// `KindFilter` is the per-registration kind filter used by `external_event_sink`
// and any consumer that needs to match events by kind. Exported from the
// canonical `actor::kind_filter` module.
pub use actor::KindFilter;

// ── Step 11 final — `nmp-ffi` re-export surface ────────────────────────────
//
// The standalone `nmp-ffi` crate (extracted from `nmp-core::ffi`) reaches
// these symbols through `nmp_core::__ffi_internal::*`. The module is
// `#[doc(hidden)]` — no app crate or library consumer should import it; the
// only legitimate consumer is `nmp-ffi`. Adding a new item here is a layer-
// shape concession (the substrate item was previously crate-private), not a
// public API addition.
//
// Why the special module rather than promoting each item to `pub` at the
// crate root: keeps the public surface area visibly identical to before the
// extraction, and gives `cargo doc` users a single place to spot "this is
// an extraction seam, not a real API".
// Gated on `feature = "native"` because the re-exports below pull in
// `run_actor_with_observers` and friends from `crate::actor`, which are
// themselves `#[cfg(feature = "native")]`. The wasm32 build
// (`--no-default-features`) has no actor thread and no FFI shell consuming
// this module.
#[cfg(feature = "native")]
#[doc(hidden)]
pub mod __ffi_internal {
    pub use crate::actor::{
        has_role, new_bunker_handshake_slot, new_event_observer_slot, new_lifecycle_observer_slot,
        new_signer_state_slot, nostrconnect_relay_url,
        activate_observer, register_c_observer, register_rust_observer,
        register_rust_observer_muted,
        run_actor_with_observers, unregister_observer,
        ActorChannels, ActorConfigSources, ActorRuntimeSlots,
        KernelEventObserverRegistration, KernelEventObserverSlot, LifecycleObserverFn,
        LifecycleObserverRegistration, LifecycleObserverSlot, LIFECYCLE_PHASE_BACKGROUND,
        LIFECYCLE_PHASE_FOREGROUND,
    };
    // `ActorMail` is the raw inbox discriminant used by `nmp-ffi::nmp_app_new`
    // to create the mpsc channel that feeds the actor thread. Not part of the
    // stable public surface (#1608); exposed only through this sealed seam so
    // the FFI layer can construct the channel without making `ActorMail` a
    // general API export.
    pub use crate::actor::ActorMail;
    // V-38: `WalletStatusSlot` / `new_wallet_status_slot` moved to `nmp-nip47`.
    // The host (per-app crate) constructs the slot itself and registers the typed
    // `"wallet"` sidecar via `register_typed_snapshot_projection` (ADR-0037).
    pub use crate::app::KernelAction;
    pub use crate::capability_socket::{
        capability_error_envelope, dispatch_capability, new_capability_callback_slot,
        CapabilityCallback, CapabilityCallbackRegistration, CapabilityCallbackSlot,
    };
    pub use crate::kernel::{
        default_registry, is_hex_id, is_hex_pubkey, new_app_relay_slot,
        new_snapshot_projection_slot, routing_trace, ActionExecuteFailure, ActionFailureKind,
        ActionRegistry, LifecyclePhase, SnapshotProjectionSlot,
    };
    // ADR-0037: the typed-projection closure type; `nmp-ffi` reaches it
    // through this internal surface to type the
    // `register_typed_snapshot_projection` seam on the `AppHost` trait.
    pub use crate::kernel::snapshot_registry::TypedProjectionFn;
    // Blocker C: `nmp-ffi` reads the admission result to record a truthful
    // composition-ledger disposition (Installed / Replaced / DroppedFull).
    pub use crate::kernel::snapshot_registry::TypedAdmission;
    // Blocker C test support: the D5 cap constant so the over-cap test can
    // fill the registry to exactly the ceiling without hard-coding the value.
    pub use crate::kernel::snapshot_registry::bounds::MAX_SNAPSHOT_PROJECTIONS;
    pub use crate::relay::{DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT};
}

/// Test-support facade: gives live-bench binaries access to the actor
/// internals without exposing domain nouns in the stable `nmp-core` API.
///
/// Enable with `features = ["test-support"]` in `Cargo.toml`.  This gate is
/// intentionally `any(test, feature = "test-support")` so `cargo test` always
/// has access without an explicit feature flag.
///
/// V-01 Phase 1c: the facade re-exports `run_actor` and the conformance
/// harness — both live on the native runtime — so the whole module is gated
/// behind `native` as well. Under `--no-default-features` there is no actor
/// thread to spawn and no harness handlers to drive.
#[cfg(all(any(test, feature = "test-support"), feature = "native"))]
pub mod testing {
    pub use crate::actor::{run_actor, ActorCommand};
    pub use crate::kernel::{PROCESS_PROJECTIONS_CHANGED, PROCESS_PROJECTIONS_SERIALIZED, PROCESS_RAM_EVENTS_EVICTED, PROCESS_STORE_LRU_EVICTED};
    pub use crate::store::{RawEvent, VerifiedEvent}; // ADR-0055 churn

    /// NIP golden-tag conformance harness — drives the (crate-private) command
    /// handlers against a real `Kernel` + `IdentityRuntime` and returns the
    /// emitted `EVENT` JSON so an integration test can assert per-kind tag
    /// structure. See `tests/nip_tag_conformance.rs`.
    pub use crate::actor::ConformanceHarness;

    use std::{sync::mpsc, thread};

    /// Spawn the kernel actor on a dedicated thread.
    ///
    /// Returns a command sender and an update receiver.  The caller drives the
    /// actor by sending [`ActorCommand`] values and reads FlatBuffers update
    /// frames from the update channel.  Dropping the sender or sending
    /// [`ActorCommand::Shutdown`] stops the actor thread.
    pub fn spawn_actor() -> (
        crate::CommandSender,
        mpsc::Receiver<crate::update_envelope::UpdateFrameBytes>,
    ) {
        // ADR-0050 §D3a — one waking inbox of `ActorMail`. The host handle and
        // the actor's self-feedback handle are both `CommandSender`s over this
        // one channel, so any command send wakes the actor.
        let (inbox_tx, command_rx) = mpsc::channel::<crate::ActorMail>();
        let (update_tx, update_rx) = mpsc::channel();
        let command_tx = crate::CommandSender::new(inbox_tx);
        // Hand the actor a clone of the command sender so dispatch arms
        // that spawn workers (currently the LNURL-pay round-trip) can
        // send follow-up `ActorCommand`s back into the loop. The outer
        // returned `command_tx` is the host's primary handle; this clone
        // serves only the actor's internal self-feedback path.
        let actor_command_tx_self = command_tx.clone();
        thread::spawn(move || run_actor(command_rx, actor_command_tx_self, update_tx));
        (command_tx, update_rx)
    }

    /// Spawn the kernel actor with a pre-set LMDB storage path.
    ///
    /// Identical to [`spawn_actor`] but writes `storage_path` into the slot
    /// before the actor thread reads it, so `Kernel::with_storage_path` picks
    /// it up at construction time (requires the `lmdb-backend` feature in
    /// `nmp-core`).  Used by the W9 A3 restart-persistence acceptance test.
    #[cfg(feature = "lmdb-backend")]
    pub fn spawn_actor_with_storage_path(
        storage_path: &str,
    ) -> (
        crate::CommandSender,
        mpsc::Receiver<crate::update_envelope::UpdateFrameBytes>,
    ) {
        use crate::actor::{run_actor_with_observers, ActorChannels, ActorConfigSources, ActorRuntimeSlots};
        use crate::slots::new_storage_path_slot;
        use std::sync::{atomic::AtomicU64, Arc, Mutex};

        let (inbox_tx, command_rx) = mpsc::channel::<crate::ActorMail>();
        let (update_tx, update_rx) = mpsc::channel();
        let command_tx = crate::CommandSender::new(inbox_tx);
        let actor_command_tx_self = command_tx.clone();

        // Pre-populate the storage path slot so the actor reads it at startup.
        let path_slot = new_storage_path_slot();
        *path_slot.lock().expect("storage_path slot") = Some(storage_path.to_string());

        // All other slots are throwaways matching the pattern in run_actor().
        thread::spawn(move || {
            let runtime = ActorRuntimeSlots {
                lifecycle_observer: crate::actor::new_lifecycle_observer_slot(),
                event_observers: crate::actor::new_event_observer_slot(),
                snapshot_projections: crate::kernel::new_snapshot_projection_slot(),
                bunker_handshake: crate::actor::new_bunker_handshake_slot(),
                signer_state: crate::actor::new_signer_state_slot(),
                bunker_hook: crate::new_bunker_hook_slot(),
                external_signer_hook: crate::new_external_signer_hook_slot(),
                configured_relays: crate::kernel::new_app_relay_slot(),
                mls_local_nsec: Arc::new(Mutex::new(None)),
                active_local_keys: Arc::new(Mutex::new(None)),
                capability_callback: crate::capability_socket::new_capability_callback_slot(),
                queue_depth: Arc::new(AtomicU64::new(0)),
                routing_trace: Arc::new(Mutex::new(None)),
                active_account: crate::slots::new_active_account_slot(),
                event_store: crate::slots::new_event_store_slot(),
                pull_cursor_registry: crate::slots::new_pull_cursor_registry_handle_slot(),
                external_event_sink_dispatcher: crate::substrate::new_external_event_sink_dispatcher_slot(),
            };
            let config = ActorConfigSources {
                storage_path: path_slot,
                coverage_hook: Arc::new(Mutex::new(None)),
                req_frame_interceptor: crate::substrate::new_req_frame_interceptor_slot(),
                host_op_handler: crate::substrate::new_host_op_handler_slot(),
                relay_text_interceptor: crate::substrate::new_relay_text_interceptor_slot(),
                relay_connected_hook: crate::substrate::new_relay_connected_hook_slot(),
                ingest_dispatcher: Arc::new(std::sync::RwLock::new(crate::substrate::EventIngestDispatcher::new())),
                dm_inbox_relays: Arc::new(Mutex::new(crate::substrate::empty_dm_inbox_relay_lookup())),
                profile_lookup: Arc::new(Mutex::new(crate::substrate::empty_profile_lookup())),
                contacts_lookup: Arc::new(Mutex::new(crate::substrate::empty_contacts_lookup())),
                blocked_relays: Arc::new(Mutex::new(crate::substrate::empty_blocked_relay_lookup())),
                bootstrap_self_kinds: Arc::new(Mutex::new(None)),
                routing_substrate: crate::slots::new_routing_substrate_slot(),
                publish_resolver: crate::slots::new_publish_resolver_slot(),
                external_event_sink_policy: crate::slots::new_external_event_sink_policy_slot(),
                kernel_clock: crate::slots::new_kernel_clock_slot(),
                gc_budget_ceiling: None,
            }
            .snapshot();
            run_actor_with_observers(
                ActorChannels { inbox_rx: command_rx, command_tx_self: actor_command_tx_self, update_tx },
                config,
                runtime,
            );
        });
        (command_tx, update_rx)
    }

    /// Build `count` real Schnorr-signed kind-1 events and enqueue them for
    /// ingest via `ActorCommand::IngestPreVerifiedEvents`.
    ///
    /// Uses a single `nostr::Keys::generate()` fixture key so all events share
    /// one pubkey — sufficient for harness pressure tests (S4/S5) where the
    /// goal is emit throughput, not per-author diversity.
    ///
    /// Schnorr sign cost: ~30–50 µs/event.  For S4 (500 events) and S5 (200
    /// events) this is 10–25 ms total — acceptable.  For S3 (100k events) use
    /// `nmp_app_inject_pre_verified_events` which uses `from_raw_unchecked`.
    #[allow(clippy::result_large_err)] // ActorCommand is large by design; boxing here would cascade through test callers
    pub fn inject_signed_events(
        tx: &crate::CommandSender,
        base_ts: u64,
        count: u32,
    ) -> Result<(), crate::CommandSendError> {
        use nostr::{EventBuilder, Keys, Timestamp};

        // Single fixture key: generate once, sign all events with it.
        // The key is not reused across harness runs (Keys::generate() uses OsRng).
        let keys = Keys::generate();
        let events: Vec<VerifiedEvent> = (0..count as u64)
            .filter_map(|i| {
                let content = format!("signed harness event {i}");
                let ts = Timestamp::from(base_ts.saturating_add(i));
                let nostr_event = EventBuilder::text_note(content)
                    .custom_created_at(ts)
                    .sign_with_keys(&keys)
                    .ok()?;
                // Convert nostr::Event to our RawEvent, then verify the full path.
                // try_from_raw re-verifies the signature — confirms the signed event
                // is well-formed before the kernel ingests it.
                let raw = RawEvent {
                    id: nostr_event.id.to_hex(),
                    pubkey: nostr_event.pubkey.to_hex(),
                    created_at: nostr_event.created_at.as_secs(),
                    kind: nostr_event.kind.as_u16() as u32,
                    tags: nostr_event
                        .tags
                        .iter()
                        .map(|t| t.as_slice().to_vec())
                        .collect(),
                    content: nostr_event.content.clone(),
                    sig: nostr_event.sig.to_string(),
                };
                VerifiedEvent::try_from_raw(raw).ok()
            })
            .collect();
        tx.send(ActorCommand::IngestPreVerifiedEvents(events))
    }

    /// Send a [`ActorCommand::Barrier`] and block until the actor acknowledges
    /// it (V-105). Returns `true` when the ack arrives before `timeout`, or
    /// `false` on timeout / disconnected channel.
    ///
    /// Sending `Barrier` after a batch of commands and waiting for the ack is
    /// the deterministic replacement for blind `recv_timeout` drain loops:
    /// the ack fires only once the actor has dispatched every command that
    /// preceded the barrier on the channel, so when `wait_barrier` returns
    /// `true` the actor's state reflects all prior commands.
    pub fn wait_barrier(tx: &crate::CommandSender, timeout: std::time::Duration) -> bool {
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        if tx.send(ActorCommand::Barrier { ack: ack_tx }).is_err() {
            return false;
        }
        ack_rx.recv_timeout(timeout).is_ok()
    }
}
