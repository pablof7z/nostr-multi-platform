//! Path-A raw C FFI surface. `mod.rs` carries the lifecycle wrappers + shared
//! argument helpers; `identity` carries the T66a identity / multi-account /
//! relay-edit wrappers; `publish` carries the publish-handle entry points
//! (signed/unsigned event publish, retry, cancel) — split out of `identity`
//! per AGENTS.md "co-locate by owner, not by role"; `timeline` carries the
//! open/close + profile claim/release wrappers; `testing` carries the
//! cfg-gated injectors (split to keep each file under the 300-LOC soft cap).

mod action;
mod capability;
mod event_observer;
mod feed;
mod identity;
mod lifecycle;
mod publish;
mod raw_event_tap;
#[cfg(feature = "signer-broker")]
mod signer_broker;
// V-51 phase 2 — routing-trace FFI snapshot accessor
// (`nmp_app_recent_routing_decisions`). Pull-only diagnostic surface; not
// folded into the snapshot tick.
mod routing_trace;
mod snapshot;
mod timeline;
// V-38: the `nmp_app_wallet_*` FFI symbols stay here as thin shims that
// route through `nmp_app_dispatch_action` for the `nmp.wallet.*` namespaces.
// The wallet runtime + the action modules + the `nmp-nwc` dep all live in
// `crates/nmp-nip47`; this module ships zero protocol code.
mod wallet;

#[cfg(any(test, feature = "test-support"))]
mod testing;

// ── Native re-export surface ──────────────────────────────────────────────
// Hoist every per-submodule FFI entry-point into the `ffi::` namespace so
// any native (non-WASM) Rust-side caller — third-party app crates
// (`nmp-app-fixture`, `nmp-app-*`), out-of-crate integration tests, the
// Android JNI shim — can name them through the rlib without an `extern "C"`
// block. The symbols themselves stay `#[no_mangle] extern "C"` in their
// owning submodules, so the Swift/C ABI is unaffected; the `pub use` only
// affects Rust-side reach.
//
// Gated on `native` (the default feature) so wasm32 (`--no-default-features`)
// continues to compile without these symbols. `android-ffi` implies `native`
// (see [features] in Cargo.toml), so the Android JNI surface inherits this
// block — the small `android-ffi` delta below adds only the four symbols
// that are android-only (account removal, bunker sign-in, full-actor stop,
// active-account switch). Likewise `test-support` implies `native` in
// practice (the `ffi` module itself is `#[cfg(feature = "native")]`), so the
// test-support delta only adds the harness-only injectors / ack / read
// helpers.
//
// `allow(unused_imports)`: in-crate `tests` modules reach these symbols by
// their `super::` / module path, so the facade re-export is only consumed by
// out-of-crate clients; keeps `cargo test -p nmp-core --lib` clean.
#[cfg(feature = "native")]
#[allow(unused_imports)]
pub use action::{nmp_app_dispatch_action, nmp_app_register_action_result_observer};
#[cfg(feature = "native")]
pub use capability::{
    nmp_app_dispatch_capability, nmp_app_free_string, nmp_app_set_capability_callback,
};
#[cfg(feature = "native")]
pub use event_observer::{nmp_app_register_event_observer, nmp_app_unregister_event_observer};
#[cfg(feature = "native")]
pub use feed::nmp_app_load_older_feed;
#[cfg(feature = "native")]
pub use identity::{
    nmp_app_add_relay, nmp_app_create_new_account, nmp_app_open_timeline, nmp_app_remove_account,
    nmp_app_remove_relay, nmp_app_signin_bunker, nmp_app_signin_nsec, nmp_app_switch_active,
};
#[cfg(feature = "native")]
#[allow(unused_imports)]
pub use lifecycle::{
    nmp_app_is_alive, nmp_app_lifecycle_background, nmp_app_lifecycle_foreground,
    nmp_app_set_lifecycle_callback,
};
// Publish-lifecycle control-plane FFI (retry/cancel). The one-door-per-
// capability rule deleted the bespoke event-producing siblings
// (`nmp_app_publish_signed_event` / `nmp_app_publish_signed_event_to` /
// `nmp_app_publish_unsigned_event`) — every event-producing publish now
// goes through `nmp_app_dispatch_action` (`nmp.publish`). Retry/cancel
// address a publish *handle* (not an event) and have no `dispatch_action`
// equivalent, so they stay on these dedicated symbols (the D11 lint
// whitelists them).
#[cfg(feature = "native")]
pub use publish::{nmp_app_cancel_publish, nmp_app_retry_publish};
#[cfg(feature = "native")]
pub use raw_event_tap::{
    nmp_app_register_raw_event_observer, nmp_app_unregister_raw_event_observer,
};
// V-51 phase 2 — routing-trace JSON accessor. Pull-only; the returned
// pointer is heap-owned and must be freed via `nmp_app_free_string`.
#[cfg(feature = "native")]
#[allow(unused_imports)]
pub use routing_trace::nmp_app_recent_routing_decisions;
#[cfg(feature = "signer-broker")]
pub use signer_broker::{
    nmp_app_cancel_bunker_handshake, nmp_app_nostrconnect_uri, nmp_broker_free_string,
    nmp_signer_broker_init,
};
#[cfg(feature = "native")]
#[allow(unused_imports)]
pub use snapshot::nmp_app_register_snapshot_projection;
#[cfg(feature = "native")]
pub use timeline::{
    nmp_app_claim_profile, nmp_app_close_author, nmp_app_close_thread, nmp_app_open_author,
    nmp_app_open_firehose_tag, nmp_app_open_thread, nmp_app_open_uri, nmp_app_release_profile,
};

// ── test-support delta ───────────────────────────────────────────────────
// Live-bench harnesses (`live-bench`) and integration test binaries
// (`nmp-testing`) need a few extra entry points that production app crates
// do not — per-action stage acks (action-FSM tests), pre-verified event
// injection (S3/S4/S5 throughput harnesses), and read-side projection JSON
// dumps (assert reducer output without going through the snapshot
// callback). Kept gated on test-support so they don't pollute the
// production-app re-export surface above.
#[cfg(any(test, feature = "test-support"))]
pub use action::nmp_app_ack_action_stage;
#[cfg(any(test, feature = "test-support"))]
pub use testing::{
    nmp_app_inject_pre_verified_events, nmp_app_inject_signed_event_json,
    nmp_app_inject_signed_events, nmp_app_read_projection_json,
};

// ── android-ffi delta ────────────────────────────────────────────────────
// `nmp_app_remove_account`, `nmp_app_signin_bunker`, and `nmp_app_switch_active`
// were historically gated here; they are lifecycle essentials every native app
// needs and are now included unconditionally in the `native` block above.
// The android-ffi identity block is intentionally removed.
// V-38: wallet FFI shims re-exported on native (default) so iOS/desktop
// callers and the Android JNI shim both pick them up via the standard
// `nmp_app_*` re-export pattern.
#[cfg(feature = "native")]
pub use wallet::{nmp_app_wallet_connect, nmp_app_wallet_disconnect, nmp_app_wallet_pay_invoice};

// Step 11 final — the FFI shell was extracted from `nmp-core::ffi` into
// this crate (`nmp-ffi`). nmp-core re-exports the items the shell reaches
// for through `nmp_core::__ffi_internal::*` (substrate-grade slot
// constructors, registration helpers, default constants); everything
// already on the public surface comes through `nmp_core::*` directly.
use nmp_core::__ffi_internal::{
    default_registry, dispatch_capability, has_role, new_bunker_handshake_slot,
    new_capability_callback_slot, new_event_observer_slot, new_lifecycle_observer_slot,
    new_raw_event_observer_slot, new_relay_edit_rows_slot, new_snapshot_projection_slot,
    nostrconnect_relay_url, register_rust_observer, register_rust_raw_observer,
    run_actor_with_observers, unregister_observer, unregister_raw_observer, ActionRegistry,
    CapabilityCallbackSlot, KernelEventObserverSlot, LifecycleObserverSlot, RawEventObserverSlot,
    SnapshotProjectionSlot, DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT,
};
// V-38: the `new_wallet_status_slot` re-export moved to `nmp-nip47`; the
// host (per-app crate) constructs the slot and registers it via
// `register_snapshot_projection("wallet", …)` itself.
use nmp_core::slots::{
    new_active_local_keys_slot, new_dm_inbox_observer_id_slot, new_mls_local_nsec_slot,
    new_publish_resolver_slot, new_routing_substrate_slot, new_routing_trace_slot,
    new_singleton_event_observer_id_slot, new_storage_path_slot, ActiveLocalKeysSlot,
    DmInboxObserverIdSlot, MlsLocalNsecSlot, PublishResolverSlot, RoutingSubstrateSlot,
    RoutingTraceSlot, SingletonEventObserverIdSlot, StoragePathSlot,
};
use nmp_core::subs::PlanCoverageHook;
use nmp_core::{
    ActorCommand, KernelEventObserver, KernelEventObserverId, KindFilter, RawEventObserver,
    RawEventObserverId,
};
use std::ffi::{c_char, c_uint, c_void, CStr};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use zeroize::Zeroizing;

type UpdateCallback = extern "C" fn(*mut c_void, *const u8, usize);

#[derive(Clone, Copy)]
struct UpdateCallbackRegistration {
    context: usize,
    callback: UpdateCallback,
}

// Step 11 final — the slot type aliases + constructors that used to live
// here (MlsLocalNsecSlot, ActiveLocalKeysSlot, StoragePathSlot,
// RoutingTraceSlot, RoutingSubstrateSlot, DmInboxObserverIdSlot,
// SingletonEventObserverIdSlot, and their `new_*_slot` constructors) moved
// to `nmp-core::slots` so the actor (a crate-private module inside
// `nmp-core`) can still name them after the FFI extraction. The aliases
// are imported through the `use nmp_core::slots::*` block at the top of
// this file.

/// Typed slot for the C-ABI update callback registration.
///
/// Written by [`nmp_app_set_update_callback`]; read by the actor thread's
/// update-listener closure. Module-private: `UpdateCallbackRegistration` is
/// also module-private so the alias cannot be wider. Lives in this crate
/// (not `nmp-core::slots`) because the `UpdateCallback` C-ABI function
/// pointer type is a structurally-FFI shape; the actor reads through this
/// slot but the type itself is a C-ABI surface concern. The byte pointer is
/// borrowed only for the callback duration; hosts must copy before returning.
type UpdateCallbackSlot = Arc<Mutex<Option<UpdateCallbackRegistration>>>;

fn new_update_callback_slot() -> UpdateCallbackSlot {
    Arc::new(Mutex::new(None))
}

pub struct NmpApp {
    tx: Sender<ActorCommand>,
    update_callback: UpdateCallbackSlot,
    capability_callback: CapabilityCallbackSlot,
    /// T118 / G3 — lifecycle observer slot. Shared `Arc` with the actor
    /// thread: registrations through [`lifecycle::nmp_app_set_lifecycle_callback`]
    /// are visible to the actor without crossing the FFI on each event.
    lifecycle_observer: LifecycleObserverSlot,
    /// T146 — kernel event observer slot. Shared `Arc` with the actor
    /// thread (and thus the kernel, which `crate::actor::run_actor_with_
    /// observers` binds onto the kernel via `set_event_observers_handle`).
    /// Per-app crates (e.g. a per-app crate) reach this slot through
    /// [`NmpApp::register_event_observer`] /
    /// [`NmpApp::unregister_event_observer`]; the C-ABI variant goes
    /// through `ffi::event_observer::nmp_app_register_event_observer`. Both
    /// paths mutate the same `Mutex<…>` the actor reads.
    event_observers: KernelEventObserverSlot,
    /// Raw signed-event tap slot. Shared `Arc` with the actor thread (and
    /// thus the kernel, which `run_actor_with_observers` binds via
    /// `set_raw_event_observers_handle`). Per-app crates reach this through
    /// [`NmpApp::register_raw_event_observer`] /
    /// [`NmpApp::unregister_raw_event_observer`]; the C-ABI variant goes
    /// through `ffi::raw_event_tap::nmp_app_register_raw_event_observer`.
    /// Both paths mutate the same `Mutex<…>` the actor reads. Delivers the
    /// verbatim flat NIP-01 signed event (`sig` included), kind-filtered.
    raw_event_observers: RawEventObserverSlot,
    /// Previously-installed NIP-17 DM-inbox raw-event observer id, for the
    /// idempotent re-invoke contract on the per-app crate's
    /// `register_dm_inbox` entry point. The per-app crate (e.g. `nmp-app-chirp`)
    /// swaps the slot atomically via [`Self::swap_nip17_dm_inbox_observer`]:
    /// the previous id is taken out, the new observer is registered, and the
    /// new id is stored back — so a re-invoke unregisters the prior observer
    /// before installing the new one, instead of stacking a fresh observer on
    /// every sign-in / account-switch cycle.
    ///
    /// Protocol-named (not chirp-named) because it tracks a NIP-17 surface;
    /// any host wiring a single DM-inbox per app shares this contract. A
    /// multi-inbox host would need a handle-returning variant instead.
    nip17_dm_inbox_observer_id: DmInboxObserverIdSlot,
    /// Singleton kernel-event observer-id slot used by per-app crates that
    /// register exactly one auxiliary `KernelEventObserver` per app and want
    /// the registration to be idempotent across re-invokes — see
    /// [`Self::swap_singleton_event_observer`]. The per-app crate swaps the
    /// slot atomically: the previous id is taken out, the new observer is
    /// registered, and the new id is stored back — so a re-invoke
    /// unregisters the prior observer before installing the new one,
    /// instead of stacking a fresh observer on every re-entry.
    ///
    /// Substrate-generic (kernel-level): the slot holds a bare
    /// [`KernelEventObserverId`]; the per-app crate decides what protocol
    /// surface uses it (D0 — the kernel never names the app noun). The
    /// first internal consumer is `nmp-app-chirp`'s per-app group-chat
    /// registration. A host that wants to keep N projections live in
    /// parallel still needs a handle-returning variant.
    singleton_event_observer_id: SingletonEventObserverIdSlot,
    /// Shared relay-edit rows handle. Cloned to the actor thread and bound
    /// onto the kernel so external Rust callers (e.g. per-app crates) can read
    /// the user's current relay list without crossing FFI.
    ///
    /// The slot is a typed [`nmp_core::RelayEditRowsSlot`]
    /// (`Arc<Mutex<RelayEditRowList>>`) — D14 forbids new bare
    /// `Arc<Mutex<Vec<…>>>` fields on `NmpApp` and the typed wrapper makes
    /// the slot's purpose visible at the declaration site.
    relay_edit_rows: nmp_core::RelayEditRowsSlot,
    /// Raw bech32 nsec (`nsec1…`) for app crates that need local key material
    /// for MLS (ADR-0025 exception; only the nmp-marmot crate holds the D13
    /// doctrine-allow). The actor thread writes this after every identity
    /// mutation that changes the active local key (create, sign-in, switch,
    /// remove). Remote-signer accounts leave this `None`. Per-app crates
    /// read it via [`NmpApp::mls_local_nsec`] so they can register a signer
    /// without Swift ever seeing the key.
    ///
    /// ADR-0025 exception: only MLS-based app crates need the raw nsec.
    /// NIP-17 DMs must NOT read this slot.
    ///
    /// Wrapped in [`Zeroizing`] so the bech32 secret is wiped from the heap
    /// when the slot is overwritten or the app drops — a plain `String` would
    /// leave the key recoverable in freed memory.
    mls_local_nsec: MlsLocalNsecSlot,
    /// Active account's local `nostr::Keys`, or `None` for a remote-signer
    /// (NIP-46 / bunker) account. The actor thread writes this after every
    /// identity mutation that changes the active local key (create, sign-in,
    /// switch, remove) — exactly parallel to `mls_local_nsec`.
    ///
    /// Substrate-generic: the actor (in `nmp-core`) names no NIP when
    /// writing this slot. The accessor [`NmpApp::active_local_keys`] is
    /// consumed by per-app crates that need the in-process keypair (today:
    /// `nmp-nip17`'s `DmInboxProjection` for NIP-44 gift-wrap unsealing,
    /// `nmp-nip57`'s zap-receipt runtime for the active pubkey). It is
    /// DISTINCT from `mls_local_nsec`: that field is the ADR-0025 bounded
    /// exception for MLS (raw bech32 nsec, D13-policed), and the ADR
    /// explicitly scopes the exception.
    ///
    /// `nostr::Keys` is `Clone` and zeroizes its own secret on drop, so no
    /// `Zeroizing` wrapper is needed here.
    active_local_keys: ActiveLocalKeysSlot,
    /// FFI-supplied persistent storage directory for the LMDB `EventStore`
    /// backend. Set by [`nmp_app_set_storage_path`] before
    /// [`nmp_app_start`]. Shared `Arc` with the actor thread: the C-ABI
    /// setter writes through this clone, the actor reads its clone when it
    /// constructs the kernel (`run_actor_with_observers` →
    /// `Kernel::with_storage_path` → `build_event_store`).
    ///
    /// `None` (the default until a host calls the setter) keeps the
    /// in-memory store. The path is only honoured when the crate is built
    /// with `--features lmdb-backend`; otherwise it is inert.
    storage_path: StoragePathSlot,
    /// V-51 phase 4 — slot the actor publishes the kernel's
    /// `RoutingTraceProjection` clone into right after kernel construction.
    /// Per-app crates (chirp-repl, the `nmp-testing` validation harness)
    /// read it through [`NmpApp::routing_trace`] to inspect the most recent
    /// routing decisions made by the kernel-side default router (or any
    /// production router an app injects via `Kernel::set_routing`, since
    /// production composition is expected to thread the same projection
    /// through the injected router's `with_trace_observer` builder).
    routing_trace: RoutingTraceSlot,
    /// V-51 phase 5 — per-app substrate-routing factory slot. The per-app
    /// crate (today: `nmp_app_chirp::ffi::register::nmp_app_chirp_register`)
    /// writes a closure here via [`Self::set_routing_substrate`]; the actor
    /// reads it after kernel construction (and again after `Reset`) and
    /// invokes [`crate::kernel::Kernel::set_routing`] with the produced
    /// `(Arc<dyn OutboxRouter>, Arc<dyn MailboxCache>)` pair. `None` (the
    /// default) leaves the kernel's `EmptyOutboxRouter` + (test-only)
    /// `TestInMemoryMailboxCache` defaults in place (substrate-honest
    /// debt B, 2026-05-24).
    routing_substrate: RoutingSubstrateSlot,
    /// Spec §271 (2026-05-25) — per-app substrate-publish-resolver
    /// factory slot. The per-app crate (today:
    /// `nmp_app_template::register_defaults`) writes a closure here via
    /// [`Self::set_publish_resolver_factory`]; the actor reads it after
    /// kernel construction (and again after `Reset`) and invokes
    /// [`crate::kernel::Kernel::set_publish_resolver`] with the produced
    /// `Arc<dyn nmp_core::publish::OutboxResolver>`. `None` (the default)
    /// leaves the kernel's `NoopOutboxResolver` default in place — every
    /// publish through `PublishTarget::Auto` then surfaces `NoTargets`
    /// (fail-closed). Mirrors `routing_substrate` exactly so the actor
    /// pair-applies both factories in one block.
    publish_resolver: PublishResolverSlot,
    /// One-shot account-creation intent: when true, the app-level MLS
    /// composition layer should publish a key package after it registers the new
    /// local identity. Kept beside the app handle because `nmp-core` owns the
    /// single account-creation FFI verb while app crates own MLS details.
    ///
    /// A bare `AtomicBool` — this flag is only ever read/written through
    /// `&self` accessors on this struct and is never shared with the actor
    /// thread (unlike the `Arc<Mutex<…>>` observer/storage slots, no clone is
    /// handed to `run_actor_with_observers`). A `Mutex<bool>` would be the
    /// wrong primitive for a single-shot lock-free flag, and the `Arc` would
    /// be dead shared ownership nothing consumes.
    pending_mls_autopublish: AtomicBool,
    actor: Mutex<Option<JoinHandle<()>>>,
    update_listener: Mutex<Option<JoinHandle<()>>>,
    /// M6 — namespace-keyed action-dispatch registry backing
    /// [`action::nmp_app_dispatch_action`]. Holds only stateless ZST module
    /// adapters, so it is `Send + Sync` and is queried directly on the FFI
    /// thread (no actor round-trip): registered modules' `start` methods are
    /// pure validators. The `Kernel`-side wiring (execution + the durable
    /// action ledger) is the M6 follow-up; see
    /// [`crate::kernel::action_registry`].
    action_registry: ActionRegistry,
    /// Host-extensible snapshot output registry — the output-side counterpart
    /// to `action_registry`. Shared `Arc<Mutex<…>>` with the actor thread
    /// (bound onto the kernel via `set_snapshot_projection_handle`): a host
    /// registers a projection closure through
    /// [`Self::register_snapshot_projection`] / the C-ABI
    /// `nmp_app_register_snapshot_projection`, and the kernel runs every
    /// registered closure in `make_update`, appending the result to
    /// `KernelSnapshot::projections`. Unlike `action_registry`, this is NOT
    /// queried on the FFI thread — it fires from inside the actor tick, hence
    /// the shared-`Arc` slot rather than a plain owned field.
    snapshot_projections: SnapshotProjectionSlot,
    /// Reusable feed-controller registry. App crates register named feed
    /// surfaces here; shells report viewport intent through generic feed FFI
    /// instead of app-specific cursor/window APIs.
    feed_registry: nmp_feed::FeedRegistrySlot,
    /// G-S4 — straddle counter for the actor command channel depth. The
    /// command channel is an unbounded `std::sync::mpsc::channel()` whose
    /// `Receiver` has no `len()`, so depth is observed indirectly: `send_cmd`
    /// (the sole funnel for FFI command sends) does `fetch_add(1)` before the
    /// `send`, and the actor does `fetch_sub(1)` per command it dequeues. The
    /// matching `Arc` clone is bound onto the kernel via
    /// `set_queue_depth_handle` so `make_update` surfaces the value as
    /// `Metrics::actor_queue_depth`. `Relaxed` ordering throughout — this is an
    /// approximate observability counter, not a synchronization primitive.
    ///
    /// Note: external sends through [`Self::actor_sender`] bypass this
    /// counter; the depth is therefore a lower bound when Rust-side runtime
    /// controllers are wired. That is acceptable for the backpressure gate,
    /// which watches for *buildup*, not exact occupancy.
    queue_depth: Arc<AtomicU64>,
    /// D2 coverage-gate hook slot. Set by the per-app crate (`nmp-app-chirp`)
    /// via [`Self::set_coverage_hook`] before `nmp_app_start`. The actor thread
    /// reads it once after kernel construction and installs it on the
    /// `SubscriptionLifecycle`. Re-installed after `Reset`. Kept in an
    /// `Arc<Mutex<Option<…>>>` slot (rather than passed directly to
    /// `run_actor_with_observers` as an `Option`) so the per-app registration
    /// pattern mirrors `storage_path` and the other pre-start slots.
    coverage_hook: Arc<Mutex<Option<PlanCoverageHook>>>,
    /// Outbound planner REQ interceptor slot. Protocol crates install this
    /// before start when they can replace selected raw REQs with a more
    /// efficient sync protocol and fall back to raw REQ when they decline.
    req_frame_interceptor: nmp_core::substrate::ReqFrameInterceptorSlot,
    /// Host-installed host-op handler slot — the substrate-generic seam app
    /// crates use to expose stateful, host-owned operations through the
    /// generic `dispatch_action` path without `nmp-core` ever naming the
    /// app's nouns (D0). See [`nmp_core::substrate::HostOpHandler`] for the full
    /// contract.
    ///
    /// Shared `Arc<Mutex<Option<Arc<dyn HostOpHandler>>>>` with the actor
    /// thread (handed to `run_actor_with_observers`): the per-app crate
    /// writes through this clone via [`Self::set_host_op_handler`] before
    /// `nmp_app_start`; the actor's `DispatchHostOp` dispatch arm reads
    /// through its clone when an `ActionModule::execute` body enqueues
    /// `ActorCommand::DispatchHostOp`. `None` (the default, and the only
    /// state for hosts that don't bind a stateful app) makes any such command
    /// record a `Failed` terminal stage — never a silent drop.
    host_op_handler: nmp_core::substrate::HostOpHandlerSlot,
    /// V-38: substrate-generic relay-text interceptor slot. A NIP-crate
    /// runtime (today `nmp-nip47`) installs itself here so the actor can
    /// peek at every inbound text frame and let the runtime decode
    /// protocol-specific responses (kind:23195 NWC). Shared `Arc` with the
    /// actor — the install side mutates through this clone, the actor reads
    /// through the matching clone in `handle_relay_event`.
    relay_text_interceptor: nmp_core::substrate::RelayTextInterceptorSlot,
    /// V-40 — shared [`nmp_core::substrate::EventIngestDispatcher`] slot.
    /// Per-NIP crates register a parser through
    /// [`Self::register_ingest_parser`] which mutates this slot under a
    /// write lock; the actor's kernel construction binds the SAME `Arc`
    /// onto the kernel so the ingest path reads through the same
    /// dispatcher the registration path mutated.
    ingest_dispatcher_slot: Arc<std::sync::RwLock<nmp_core::substrate::EventIngestDispatcher>>,
    /// V-40 — shared [`nmp_core::substrate::DmInboxRelayLookup`] slot. The
    /// per-app crate (today `nmp-nip17::register_actions`) writes a
    /// concrete `DmRelayCache` here via
    /// [`Self::set_dm_inbox_relay_lookup`]; the actor reads the current
    /// handle and binds it onto the kernel so the gift-wrap publish path
    /// and the planner-side `KernelMailboxes` adapter both see the same
    /// cache.
    dm_inbox_relays_slot: Arc<Mutex<Arc<dyn nmp_core::substrate::DmInboxRelayLookup>>>,
    /// NIP-47 wallet double-tap guard: bolt11 strings the FFI surface has
    /// already accepted for `pay_invoice` but for which the kind:23195
    /// response (or a timeout) has not yet cleared. Keyed by the full bolt11
    /// string — a same-invoice retap maps to the same key, and the FFI
    /// short-circuits before constructing a second
    /// `ActorCommand::WalletPayInvoice`.
    ///
    /// Lives entirely in the FFI layer (no actor coupling): expiry is wall-
    /// clock based — entries older than [`wallet::INFLIGHT_BOLT11_TTL`] are
    /// swept at every `nmp_app_wallet_pay_invoice` call, so a legitimate retry
    /// after the TTL passes through. The TTL is sized for "the NWC response
    /// is in flight" — long enough to absorb relay round-trip jitter, short
    /// enough that a wallet that never responds does not block the user
    /// forever.
    ///
    /// D14: `Mutex<HashMap<…>>` is NOT the banned `Arc<Mutex<Vec<…>>>` shape
    /// the rule disciplines (a `HashMap` is not a `Vec`, and the slot is not
    /// shared with the actor — no `Arc`). The simpler primitive is correct
    /// here: nothing on the actor side reads or writes this slot, so the
    /// `Arc` clone would be dead shared ownership.
    ///
    /// V-38: stays in `nmp-core::ffi` (a generic dedup guard, no protocol
    /// content) so the wallet pay-invoice FFI shim can short-circuit a
    /// same-bolt11 retap before it crosses into `nmp-nip47`.
    pub(crate) inflight_bolt11: Mutex<std::collections::HashMap<String, std::time::Instant>>,
    /// Generic dispatch idempotency guard: dedup-keys for
    /// [`action::nmp_app_dispatch_action`] calls accepted by the registry but
    /// whose action-result has not yet cleared. Keyed by a stable 64-bit
    /// FNV-1a hash of `(namespace, action_json)` (see
    /// [`nmp_core::stable_hash::stable_hash64`]) — a same-action retap inside
    /// the TTL window maps to the same key, and the FFI short-circuits before
    /// the second `start()` + executor pass enqueues a duplicate
    /// `ActorCommand`. The stored value is `(when_first_seen,
    /// original_correlation_id)`: a dedup hit returns the original id in the
    /// `{"correlation_id":...}` envelope so the host's spinner stays bound to
    /// the first dispatch (mirrors the "id stays bound" semantic of the
    /// `executor_failure_returns_correlation_id_and_enqueues_
    /// failed_terminal` test).
    ///
    /// Mirrors the [`inflight_bolt11`] pattern but for ALL action namespaces,
    /// not just `pay_invoice`. The primary motivator is the NIP-17 DM send
    /// path: a rapid re-tap on Send before the gift-wrap fan-out completes
    /// would otherwise mint a second batch of kind:1059 envelopes that are
    /// indistinguishable to recipients (no on-the-wire dedup is possible).
    /// The guard is generic by intent — every namespace the host dispatches
    /// shares the same 30-second wall-clock window.
    ///
    /// Lives entirely in the FFI layer (no actor coupling): expiry is
    /// wall-clock based — entries older than
    /// [`action::INFLIGHT_DISPATCH_TTL`] are swept at every
    /// `dispatch_action_json` call, so a legitimate retry after the TTL
    /// passes through.
    ///
    /// D14: `Mutex<HashMap<…>>` is NOT the banned `Arc<Mutex<Vec<…>>>` shape
    /// — same reasoning as `inflight_bolt11` above.
    inflight_dispatches: Mutex<std::collections::HashMap<u64, (std::time::Instant, String)>>,
    /// Idempotency guard for `nmp_app_create_new_account` (identity.rs).
    ///
    /// `nmp_app_create_new_account` bypasses `inflight_dispatches` (which
    /// covers `dispatch_action` only) and sends `ActorCommand::CreateAccount`
    /// directly. Without a guard, two rapid taps on the iOS "Create account"
    /// button mint two distinct keypairs — the second overwrites the first and
    /// the user loses the original nsec with no diagnostic signal.
    ///
    /// This slot holds the `Instant` of the last accepted dispatch. A
    /// re-call within 30 s (matching `INFLIGHT_DISPATCH_TTL`) is a no-op.
    /// After the TTL a legitimate retry (e.g. creation silently failed) is
    /// allowed through. No actor coupling required: the guard lives entirely
    /// in the FFI layer, same posture as `inflight_dispatches`.
    pub(crate) creating_account_inflight: Mutex<Option<std::time::Instant>>,
}

impl Drop for NmpApp {
    fn drop(&mut self) {
        if let Ok(mut callback) = self.update_callback.lock() {
            *callback = None;
        }
        // Route through `send_cmd` so the G-S4 queue-depth counter stays
        // consistent: the actor decrements it as it dequeues `Shutdown`.
        self.send_cmd(ActorCommand::Shutdown);
        if let Ok(mut actor) = self.actor.lock() {
            if let Some(handle) = actor.take() {
                let _ = handle.join();
            }
        }
        if let Ok(mut listener) = self.update_listener.lock() {
            if let Some(handle) = listener.take() {
                let _ = handle.join();
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn nmp_app_new() -> *mut NmpApp {
    let (command_tx, command_rx) = mpsc::channel();
    let (update_tx, update_rx) = mpsc::channel();
    let update_callback = new_update_callback_slot();
    let listener_callback = Arc::clone(&update_callback);
    // T118 / G3 — shared lifecycle observer slot. The FFI side
    // (`nmp_app_set_lifecycle_callback`) writes registrations through one
    // clone; the actor thread reads through the other when handling
    // `ActorCommand::LifecycleEvent`. Both see the same `Mutex<Option<...>>`.
    let lifecycle_observer = new_lifecycle_observer_slot();
    let actor_lifecycle_observer = Arc::clone(&lifecycle_observer);
    // T146 — shared kernel event observer slot. Same pattern as the
    // lifecycle slot: the `NmpApp` keeps one clone (used by Rust + C-ABI
    // registration entry points), the actor thread carries another for the
    // kernel's fan-out path (`set_event_observers_handle`). Registrations
    // mutate the inner `Mutex` visible to both sides.
    let event_observers = new_event_observer_slot();
    let actor_event_observers = Arc::clone(&event_observers);
    // Raw signed-event tap slot. Same shared-`Arc` pattern: the `NmpApp`
    // keeps one clone (Rust + C-ABI registration entry points), the actor
    // thread carries another for the kernel's tap path
    // (`set_raw_event_observers_handle`).
    let raw_event_observers = new_raw_event_observer_slot();
    let actor_raw_event_observers = Arc::clone(&raw_event_observers);
    // Per-app idempotency slots — track the previously-installed observer id
    // for single-instance auxiliary observer registrations a per-app crate
    // (e.g. `nmp-app-chirp`) wires through `swap_nip17_dm_inbox_observer` /
    // `swap_singleton_event_observer`. NOT shared with the actor thread —
    // the actor never reads these; only the FFI side calls the swap
    // accessors. Owned by `NmpApp`, dropped with it (so the slot dies with
    // the app — no global aliasing across `nmp_app_free`).
    let nip17_dm_inbox_observer_id = new_dm_inbox_observer_id_slot();
    let singleton_event_observer_id = new_singleton_event_observer_id_slot();
    // Host-extensible snapshot output slot. Same shared-`Arc` pattern: the
    // `NmpApp` keeps one clone (Rust + C-ABI registration entry points), the
    // actor thread carries another and binds it onto the kernel
    // (`set_snapshot_projection_handle`). Registrations mutate the inner
    // `Mutex<SnapshotRegistry>` visible to both sides.
    let snapshot_projections = new_snapshot_projection_slot();
    let actor_snapshot_projections = Arc::clone(&snapshot_projections);
    // V-38: the shared `WalletStatusSlot` + the `"wallet"` snapshot
    // projection moved to `crates/nmp-nip47`. The host (per-app crate)
    // builds those itself and calls
    // `nmp_app_register_snapshot_projection("wallet", …)` for the read side
    // + `nmp_nip47::install_wallet_runtime(handle)` for the write side. The
    // actor now only carries a substrate-generic relay-text interceptor
    // slot.
    let relay_text_interceptor = nmp_core::substrate::new_relay_text_interceptor_slot();
    let actor_relay_text_interceptor = Arc::clone(&relay_text_interceptor);
    // D0: NIP-46 remote signing is an app noun. The shared bunker-handshake
    // slot is handed to the actor: `run_actor_with_observers` both gives one
    // `Arc` clone to the actor's `IdentityRuntime` (the sole writer, D4) and
    // registers the built-in `"bunker_handshake"` snapshot-projection closure
    // that reads the other clone. Handshake state therefore reaches the host
    // through `projections["bunker_handshake"]` instead of a baked-in
    // `KernelSnapshot` field — and every actor consumer (FFI or test) gets the
    // projection without a separate FFI registration step.
    let actor_bunker_handshake = new_bunker_handshake_slot();
    // Shared relay-edit rows handle. Cloned to the actor thread and bound
    // onto the kernel so external Rust callers can read the user's current
    // relay list without crossing FFI.
    //
    // Typed slot constructor — see `kernel/relay_projection.rs`.
    let relay_edit_rows: nmp_core::RelayEditRowsSlot = new_relay_edit_rows_slot();
    let actor_relay_edit_rows = Arc::clone(&relay_edit_rows);
    // Active local (nsec) key slot. The actor updates this after every
    // identity mutation; per-app crates read via NmpApp::mls_local_nsec.
    let mls_local_nsec: MlsLocalNsecSlot = new_mls_local_nsec_slot();
    let actor_mls_local_nsec = Arc::clone(&mls_local_nsec);
    // Active local `nostr::Keys` slot — substrate-generic. Same shared-`Arc`
    // pattern as `mls_local_nsec`: the actor updates it on every identity
    // mutation; per-app crates read via `NmpApp::active_local_keys` (today:
    // `nmp-nip17` `DmInboxProjection` for NIP-44 gift-wrap unsealing,
    // `nmp-nip57` zap-receipt runtime for the active pubkey).
    let active_local_keys: ActiveLocalKeysSlot = new_active_local_keys_slot();
    let actor_active_local_keys = Arc::clone(&active_local_keys);
    // Shared capability callback slot. FFI registration writes through the
    // app clone; the actor reads through its clone when issuing keyring
    // requests during start/sign-in/create/switch/remove.
    let capability_callback = new_capability_callback_slot();
    let actor_capability_callback = Arc::clone(&capability_callback);
    // FFI-supplied LMDB storage path slot. `nmp_app_set_storage_path`
    // writes through the `NmpApp`'s clone before `nmp_app_start`; the actor
    // reads through this clone when it builds the kernel. Default `None`
    // → in-memory store.
    let storage_path: StoragePathSlot = new_storage_path_slot();
    let actor_storage_path = Arc::clone(&storage_path);
    // V-51 phase 4 — shared routing-trace projection slot. The actor
    // populates this with `kernel.routing_trace()` right after kernel
    // construction (and re-populates on `Reset`); per-app crates read it
    // through `NmpApp::routing_trace`.
    let routing_trace: RoutingTraceSlot = new_routing_trace_slot();
    let actor_routing_trace = Arc::clone(&routing_trace);
    // V-51 phase 5 — substrate-routing factory slot. Default `None`; the
    // per-app crate installs a closure via `set_routing_substrate` before
    // `nmp_app_start`. The actor reads the slot once after kernel construction
    // (and once again per `Reset`) and applies the produced router/cache via
    // `Kernel::set_routing`.
    let routing_substrate: RoutingSubstrateSlot = new_routing_substrate_slot();
    let actor_routing_substrate = Arc::clone(&routing_substrate);
    // Spec §271 (2026-05-25) — substrate-publish-resolver factory slot.
    // Default `None`; the per-app crate installs a closure via
    // `set_publish_resolver_factory` before `nmp_app_start`. The actor reads
    // the slot once after kernel construction (and once per `Reset`) and
    // applies the produced resolver via `Kernel::set_publish_resolver`.
    let publish_resolver: PublishResolverSlot = new_publish_resolver_slot();
    let actor_publish_resolver = Arc::clone(&publish_resolver);
    let feed_registry = nmp_feed::new_feed_registry_slot();
    // One-shot MLS-autopublish intent flag. Not shared with the actor thread,
    // so a bare `AtomicBool` — no `Arc`, no `Mutex` — is the right primitive.
    let pending_mls_autopublish = AtomicBool::new(false);
    // G-S4 — actor command-channel depth straddle counter. The `NmpApp` keeps
    // one `Arc` clone (incremented by `send_cmd` before every channel send);
    // the actor carries the other (decremented per command dequeued) and binds
    // it onto the kernel so `make_update` reads it. See the `queue_depth` field
    // doc on `NmpApp` for the full contract.
    let queue_depth: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
    let actor_queue_depth = Arc::clone(&queue_depth);
    // D2 — shared coverage-gate hook slot. The per-app crate (e.g.
    // `nmp-app-chirp`) writes through the `NmpApp`'s clone via
    // [`NmpApp::set_coverage_hook`] before `nmp_app_start`; the actor reads
    // its clone once after kernel construction and installs the hook on the
    // `SubscriptionLifecycle`. Re-installed by the `Reset` dispatch arm so the
    // rebuilt lifecycle also enforces D2. `None` (the test default) leaves
    // the lifecycle's `coverage_hook: None` in place — every plan flows
    // straight to raw REQ, preserving legacy behaviour.
    let coverage_hook: Arc<Mutex<Option<PlanCoverageHook>>> = Arc::new(Mutex::new(None));
    let actor_coverage_hook = Arc::clone(&coverage_hook);
    let req_frame_interceptor = nmp_core::substrate::new_req_frame_interceptor_slot();
    let actor_req_frame_interceptor = Arc::clone(&req_frame_interceptor);
    // Substrate-generic host-op handler slot — the actor's `DispatchHostOp`
    // dispatch arm reads from this clone. The per-app crate (today
    // `nmp-app-marmot`) writes through `NmpApp::set_host_op_handler` before
    // `nmp_app_start`. `None` is the default and the production state for
    // every host that does not bind a stateful app crate.
    let host_op_handler: nmp_core::substrate::HostOpHandlerSlot =
        nmp_core::substrate::new_host_op_handler_slot();
    let actor_host_op_handler = nmp_core::substrate::HostOpHandlerSlot::clone(&host_op_handler);
    // V-40 — substrate `EventIngestDispatcher` slot. Per-NIP crates
    // (today: `nmp-nip17`) register their kind parsers through
    // [`NmpApp::register_ingest_parser`] which mutates THIS slot; the
    // actor's kernel construction step binds it onto the kernel so the
    // ingest path and the registration path share one dispatcher.
    let ingest_dispatcher_slot: Arc<std::sync::RwLock<nmp_core::substrate::EventIngestDispatcher>> =
        Arc::new(std::sync::RwLock::new(
            nmp_core::substrate::EventIngestDispatcher::new(),
        ));
    let actor_ingest_dispatcher = Arc::clone(&ingest_dispatcher_slot);
    // V-40 — substrate `DmInboxRelayLookup` slot. The per-app crate
    // (today: `nmp-nip17::register_actions`) installs the concrete
    // `DmRelayCache` here via [`NmpApp::set_dm_inbox_relay_lookup`];
    // the actor's kernel construction reads the current handle and binds
    // it onto the kernel. Default is `EmptyDmInboxRelayLookup` (fail-
    // closed cold-start).
    let dm_inbox_relays_slot: Arc<Mutex<Arc<dyn nmp_core::substrate::DmInboxRelayLookup>>> =
        Arc::new(Mutex::new(
            nmp_core::substrate::empty_dm_inbox_relay_lookup(),
        ));
    let actor_dm_inbox_relays = Arc::clone(&dm_inbox_relays_slot);
    // Clone so we can report actor death through the same listener pipe.
    // The actor `move`s its own `update_tx` into `run_actor_with_observers`;
    // this clone is the supervisor's last live handle once that one is
    // dropped — it MUST outlive the inner closure so the panic frame can
    // still be delivered after the actor's own sender is gone.
    let update_tx_panic = update_tx.clone();
    // Self-feedback sender for the actor — a clone of the command sender
    // that the host also keeps (`command_tx` above). Background workers
    // spawned from dispatch arms (the LNURL-pay round-trip the NIP-57
    // `Protocol(...)` arm carries through `ProtocolCommandContext::command_sender_clone`)
    // use this clone to send follow-up `ActorCommand`s back into the loop
    // without crossing FFI.
    //
    // G-S4 caveat: sends through this clone bypass the `queue_depth`
    // straddle counter (the only incrementing path is `NmpApp::send_cmd`).
    // The `actor_queue_depth` snapshot metric is therefore a lower bound
    // for self-feedback traffic — acceptable for a backpressure gate that
    // watches for buildup, matches the existing `actor_sender()` caveat.
    let actor_command_tx_self = command_tx.clone();
    let actor = thread::spawn(move || {
        // D7 (actor-death visibility): the actor thread owns the kernel loop.
        // If it panics, `send_cmd` would otherwise silently drop every
        // subsequent command (the channel closes with no signal). Catch the
        // unwind here and emit one envelope-conforming `Panic` frame on the
        // update channel *before* this thread (and `update_tx`) is dropped,
        // so the host receives a terminal, decodable signal.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_actor_with_observers(
                command_rx,
                actor_command_tx_self,
                update_tx,
                actor_lifecycle_observer,
                actor_event_observers,
                actor_raw_event_observers,
                actor_snapshot_projections,
                // V-38: substrate-generic relay-text interceptor slot. The
                // host's `nmp-nip47` install registers its NWC runtime here;
                // the actor calls `interceptor.on_relay_text(...)` for every
                // inbound text frame.
                actor_relay_text_interceptor,
                // D0: NIP-46 remote signing is an app noun — the
                // bunker-handshake slot the actor's `IdentityRuntime` writes;
                // the `"bunker_handshake"` projection (registered below) reads
                // the matching clone.
                actor_bunker_handshake,
                actor_relay_edit_rows,
                actor_mls_local_nsec,
                actor_active_local_keys,
                actor_capability_callback,
                actor_storage_path,
                // G-S4 — the actor's clone of the command-channel depth
                // counter. Decremented per dequeued command; bound onto the
                // kernel for the `actor_queue_depth` snapshot field.
                actor_queue_depth,
                // D2 — the actor's clone of the coverage-gate hook slot. Read
                // once after kernel construction (and again after `Reset`) and
                // installed on `SubscriptionLifecycle` so the production plan
                // pipeline enforces D2 ("negentropy before REQ") via the
                // per-app crate's policy closure.
                actor_coverage_hook,
                actor_req_frame_interceptor,
                // The actor's clone of the host-op handler slot — read by the
                // `DispatchHostOp` dispatch arm. `None` (no stateful app bound)
                // makes any such command record a `Failed` terminal stage;
                // never a silent drop.
                actor_host_op_handler,
                // V-40 — the actor's clones of the substrate
                // `EventIngestDispatcher` slot and the
                // `DmInboxRelayLookup` slot. Per-NIP crates mutate the
                // shared `Arc`s via `NmpApp::register_ingest_parser` /
                // `set_dm_inbox_relay_lookup`; the actor binds them onto
                // the kernel at construction.
                actor_ingest_dispatcher,
                actor_dm_inbox_relays,
                // V-51 phase 4 — the actor's clone of the routing-trace slot.
                // Filled with `kernel.routing_trace()` right after kernel
                // construction (and re-filled on `Reset`); per-app crates read
                // it through `NmpApp::routing_trace`.
                actor_routing_trace,
                // V-51 phase 5 — the actor's clone of the substrate-routing
                // factory slot. Read once after kernel construction (and on
                // `Reset`); the factory builds the production router/cache
                // pair the actor installs via `Kernel::set_routing`.
                actor_routing_substrate,
                // Spec §271 (2026-05-25) — the actor's clone of the
                // substrate-publish-resolver factory slot. Read once after
                // kernel construction (and on `Reset`); the factory builds
                // the production resolver the actor installs via
                // `Kernel::set_publish_resolver`. `None` leaves the
                // kernel's `NoopOutboxResolver` default in place.
                actor_publish_resolver,
            );
        }));
        if let Err(e) = result {
            // Best-effort downcast of the panic payload — see
            // `update_envelope::panic_message`. D6: `panic_message` and
            // `encode_panic` is infallible in practice, so building the
            // death signal cannot itself panic.
            let msg = nmp_core::panic_message(&*e);
            let frame = nmp_core::encode_panic(format!("actor thread died: {msg}"));
            let _ = update_tx_panic.send(frame);
        }
    });
    let update_listener = thread::spawn(move || {
        while let Ok(update) = update_rx.recv() {
            let callback = listener_callback.lock().ok().and_then(|guard| *guard);
            if let Some(registration) = callback {
                // UB guard: the foreign update callback may panic / raise.
                // This listener thread has no outer `catch_unwind` (unlike
                // the actor thread above), so an unguarded unwind here is
                // undefined behaviour across the C ABI boundary.
                let _ = nmp_core::ffi_guard::guard_ffi_callback("update listener", || {
                    (registration.callback)(
                        registration.context as *mut c_void,
                        update.as_ptr(),
                        update.len(),
                    );
                });
            }
        }
    });

    let app = NmpApp {
        tx: command_tx,
        update_callback,
        capability_callback,
        lifecycle_observer,
        event_observers,
        raw_event_observers,
        nip17_dm_inbox_observer_id,
        singleton_event_observer_id,
        relay_edit_rows,
        mls_local_nsec,
        active_local_keys,
        storage_path,
        routing_trace,
        routing_substrate,
        publish_resolver,
        pending_mls_autopublish,
        actor: Mutex::new(Some(actor)),
        update_listener: Mutex::new(Some(update_listener)),
        // M6 — the action registry the kernel ships with: `PublishModule`
        // only. NIP-29 / NIP-59 modules are app nouns (D0) and are
        // registered by the app host against its own registry instance.
        action_registry: default_registry(),
        // Host-extensible snapshot output: ships with the built-in `"wallet"`
        // projection (registered below when `feature = "wallet"`). A non-social
        // host registers its own projections via
        // `nmp_app_register_snapshot_projection` during init.
        snapshot_projections,
        feed_registry,
        // G-S4 — the `NmpApp`'s clone of the command-channel depth counter,
        // incremented by `send_cmd`. The actor holds the matching clone.
        queue_depth,
        // D2 — the `NmpApp`'s clone of the coverage-gate hook slot. Written
        // by the per-app crate via [`NmpApp::set_coverage_hook`] before
        // `nmp_app_start`; the actor reads its clone after kernel
        // construction and installs the hook on `SubscriptionLifecycle`.
        coverage_hook,
        req_frame_interceptor,
        // The `NmpApp`'s clone of the host-op handler slot. Written by the
        // per-app crate (today `nmp-app-marmot`) via
        // [`NmpApp::set_host_op_handler`] before `nmp_app_start`; the actor
        // reads through its matching clone when the `DispatchHostOp` arm
        // fires.
        host_op_handler,
        // V-38: the same Arc clone the actor holds — `nmp-nip47` installs
        // its NWC runtime here.
        relay_text_interceptor,
        // V-40 — the `NmpApp`'s clones of the substrate `IngestParser`
        // dispatcher slot and the DM-inbox relay-lookup slot. Per-NIP
        // crates mutate these through
        // [`NmpApp::register_ingest_parser`] / [`NmpApp::set_dm_inbox_relay_lookup`];
        // the actor's matching clones bind onto the kernel at
        // construction time so the registration and the read path
        // share one `Arc`.
        ingest_dispatcher_slot,
        dm_inbox_relays_slot,
        // V-38: NIP-47 wallet `pay_invoice` double-tap guard. The state is
        // still in `NmpApp` (a generic UI dedup primitive, no protocol
        // content); the wallet runtime that consumes the dispatched action
        // moved to `crates/nmp-nip47`.
        inflight_bolt11: Mutex::new(std::collections::HashMap::new()),
        // Generic dispatch idempotency guard. Empty at construction; populated
        // by `ffi::action::dispatch_action_json` on each accepted dispatch,
        // swept on TTL expiry (no cross-thread coupling).
        inflight_dispatches: Mutex::new(std::collections::HashMap::new()),
        // create_account idempotency guard: None until first call; set to
        // `Some(Instant::now())` by `nmp_app_create_new_account` and
        // re-admits after 30 s (same TTL as `inflight_dispatches`).
        creating_account_inflight: Mutex::new(None),
    };

    // V-38: the `"wallet"` snapshot projection moved to `crates/nmp-nip47`.
    // The host (per-app crate) registers it themselves on the `NmpApp`
    // via `register_snapshot_projection("wallet", …)` after constructing
    // the `WalletStatusSlot` from `nmp_nip47`.
    //
    // D0 — the built-in `"bunker_handshake"` projection is registered inside
    // `run_actor_with_observers` (at the actor wiring site), not here: it
    // reads the actor-owned bunker-handshake slot, so every actor consumer
    // (FFI or test) gets the projection without a separate FFI step.

    Box::into_raw(Box::new(app))
}

impl NmpApp {
    /// Send a command to the actor thread.
    ///
    /// D6: a disconnected channel (actor thread panicked or exited) must
    /// degrade gracefully — never panic, never write to stderr from library
    /// code. The send is best-effort; the dropped command is the failure
    /// signal.
    ///
    /// D7 (actor-death visibility): if the actor thread panics, the
    /// supervisor closure in `nmp_app_new` emits one
    /// `UpdateEnvelope::Panic` frame on the update channel before the channel
    /// closes — see [`crate::update_envelope`]'s actor-death contract. So a
    /// dropped command here is no longer *silent*: the host has already
    /// received (or will receive) the terminal panic frame and is expected
    /// to surface a fatal error rather than keep sending.
    pub(crate) fn send_cmd(&self, cmd: ActorCommand) {
        // G-S4 — straddle counter: increment before the send so the kernel
        // never observes a command "in flight" with a stale-low depth. The
        // actor decrements as it dequeues. `Relaxed` is sufficient — the value
        // is approximate observability, not a synchronization edge. If the
        // send fails (actor thread gone) the command is dropped and the
        // counter is left one high; that is harmless on a dead actor.
        self.queue_depth.fetch_add(1, Ordering::Relaxed);
        let _ = self.tx.send(cmd);
    }

    /// Register a typed [`nmp_core::substrate::ActionModule`] `M` against the
    /// app's action registry — ADR-0027's single-call typed seam, and the
    /// sole host action-registration path on master.
    ///
    /// `M::start` handles validation AND `M::execute` handles execution, both
    /// under the same typed namespace (`M::NAMESPACE`): there is no possible
    /// partial-registration gap (the pre-ADR-0027 dual `register_action_module`
    /// / `register_action_executor` closure seam has been deleted).
    ///
    /// Registration MUST happen during host init — before `nmp_app_start`
    /// and before any [`action::nmp_app_dispatch_action`] call — because it
    /// requires `&mut self`.
    pub fn register_action<M: nmp_core::substrate::ActionModule + 'static>(&mut self) {
        self.action_registry.register::<M>();
    }

    /// Register a host-supplied snapshot projection — the output-side
    /// counterpart to [`Self::register_action`].
    ///
    /// The closure runs on **every snapshot tick** (inside the actor's
    /// `make_update`) and its returned JSON value is appended to
    /// `KernelSnapshot::projections` under `key`. A marketplace app registers
    /// `"market.listings"`, a todo app registers `"todo.items"` — each gets
    /// its own snapshot namespace WITHOUT editing `nmp-core`'s sealed social
    /// `KernelSnapshot` fields.
    ///
    /// Unlike [`Self::register_action`], this does NOT require `&mut self`:
    /// the registry lives behind a shared `Arc<Mutex<…>>` and the mutation is
    /// a lock-and-push. It is still intended as a host-init call.
    ///
    /// D8 — the closure runs on the actor thread inside the snapshot tick. It
    /// MUST be cheap and non-blocking (no I/O, no mutex waits): a blocking
    /// closure stalls every subsequent snapshot and freezes the host's
    /// update stream. A poisoned registry mutex is a silent no-op (D6).
    pub fn register_snapshot_projection(
        &self,
        key: impl Into<String>,
        f: impl Fn() -> serde_json::Value + Send + Sync + 'static,
    ) {
        if let Ok(mut registry) = self.snapshot_projections.lock() {
            registry.register(key, f);
        }
    }

    /// Register a reusable feed surface. The controller owns ordering,
    /// viewport state, paging, and render payload selection; native shells
    /// only render the emitted projection and report viewport intent.
    pub fn register_feed(
        &self,
        key: impl Into<String>,
        controller: std::sync::Arc<dyn nmp_feed::FeedController>,
    ) {
        let key = key.into();
        self.feed_registry
            .register(key.clone(), std::sync::Arc::clone(&controller));
        self.register_snapshot_projection(key, move || controller.snapshot_json());
    }

    #[must_use]
    pub fn load_older_feed(&self, key: &str) -> bool {
        let changed = self.feed_registry.load_older(key);
        if changed {
            self.send_cmd(ActorCommand::MarkChangedSinceEmit);
        }
        changed
    }

    /// Register a host-supplied action-result observer — the *push*
    /// counterpart to [`Self::register_snapshot_projection`]'s pull seam.
    ///
    /// After [`action::nmp_app_dispatch_action`] accepts an action and its
    /// executor returns `Ok`, the observer is handed a
    /// [`nmp_core::substrate::ActionResult`] carrying the action's
    /// `correlation_id`. This is an "action accepted and enqueued" signal,
    /// not a completion carrier — for `nmp.publish` the actor still has to
    /// verify+publish after this fires; that outcome reaches the host via
    /// the snapshot-projection (pull) path.
    ///
    /// Like `register_snapshot_projection`, this does NOT require `&mut self`:
    /// the observer lives behind a shared `Arc<Mutex<…>>` slot inside the
    /// action registry, so a host may register it before or after
    /// `nmp_app_start`. A second registration replaces the first.
    pub fn register_action_result_observer(
        &self,
        f: impl Fn(nmp_core::substrate::ActionResult) + Send + Sync + 'static,
    ) {
        self.action_registry.set_result_observer(f);
    }

    /// Install the D2 coverage-gate hook. MUST be called before
    /// [`nmp_app_start`]. The hook is a closure that receives a
    /// [`nmp_core::planner::CompiledPlan`] after M2 compile and may mutate it
    /// (e.g. prune relays or mark sub-shapes for negentropy). See
    /// [`crate::subs::PlanCoverageHook`].
    ///
    /// D0: `nmp-core` defines the seam; the assembly crate installs the policy
    /// closure (today `nmp-app-chirp` consumes [`nmp_coverage_gate::CoverageGate`]).
    ///
    /// The hook lives in an `Arc<Mutex<Option<…>>>` slot shared with the
    /// actor thread; the actor reads it once after kernel construction (and
    /// again after `Reset`) and binds it onto the `SubscriptionLifecycle`.
    /// A second call replaces the slot's contents — the next `Reset` will
    /// install the newer hook, but the currently-installed hook on the live
    /// lifecycle is not retroactively swapped.
    ///
    /// D6 — a poisoned slot mutex is a silent no-op (the host's hook is
    /// dropped); the lifecycle keeps whatever policy was previously
    /// installed (or `None`).
    pub fn set_coverage_hook(&self, hook: PlanCoverageHook) {
        if let Ok(mut slot) = self.coverage_hook.lock() {
            *slot = Some(hook);
        }
    }

    /// Install the outbound planner REQ interceptor.
    ///
    /// MUST be called before `nmp_app_start`. The actor reads this slot at
    /// kernel construction and after `Reset`; absent means every planner REQ
    /// follows the raw NIP-01 path.
    pub fn set_req_frame_interceptor(
        &self,
        interceptor: std::sync::Arc<dyn nmp_core::substrate::ReqFrameInterceptor>,
    ) {
        if let Ok(mut slot) = self.req_frame_interceptor.lock() {
            *slot = Some(interceptor);
        }
    }

    /// Install the substrate-generic [`nmp_core::substrate::HostOpHandler`].
    ///
    /// The handler is the bridge between an [`nmp_core::substrate::ActionModule`]
    /// whose `execute()` body emits [`ActorCommand::DispatchHostOp`]
    /// and the app-owned state the op mutates (today: `nmp-app-marmot`'s
    /// `MarmotService`). The actor's `DispatchHostOp` arm pulls the handler
    /// from this slot and calls `handle(action_json, correlation_id)`.
    ///
    /// `nmp-core` deliberately does NOT name the app's typed action enum
    /// (D0 — no Marmot / MLS / app-specific nouns in the kernel); the handler
    /// speaks only `&str` + [`serde_json::Value`]. The matching `ActionModule`
    /// lives in the app crate and serializes its typed action into the same
    /// JSON envelope the handler parses back out — exactly the same JSON
    /// translation layer the legacy `nmp_marmot_dispatch` symbol used
    /// (deleted in ADR-0025 PR 3, 2026-05-23).
    ///
    /// The slot is `Arc<Mutex<Option<Arc<dyn HostOpHandler>>>>` shared with
    /// the actor thread (handed to `run_actor_with_observers` at
    /// construction time). Like [`Self::set_coverage_hook`], this takes
    /// `&self`: the host may install — or replace — the handler at any
    /// time. A second call replaces the first; the new handler is the one
    /// the *next* `DispatchHostOp` arm picks up.
    ///
    /// D6 — a poisoned slot mutex is a silent no-op (the host's handler is
    /// dropped on the floor); the slot keeps whatever value was previously
    /// installed (or `None`, in which case the dispatch arm records the
    /// `Failed { reason: "no host op handler installed" }` terminal). MUST
    /// be called before any `nmp_app_dispatch_action` that targets a
    /// namespace whose `ActionModule::execute` emits `DispatchHostOp` —
    /// installing the handler late produces a stream of `Failed` terminals
    /// for the gap, not a panic.
    pub fn set_host_op_handler(
        &self,
        handler: std::sync::Arc<dyn nmp_core::substrate::HostOpHandler>,
    ) {
        if let Ok(mut slot) = self.host_op_handler.lock() {
            *slot = Some(handler);
        }
    }

    /// V-38 — install a substrate-generic [`nmp_core::substrate::RelayTextInterceptor`].
    /// Today the only consumer is `nmp-nip47`'s NWC runtime, which peeks
    /// at every inbound text frame from the wallet relay to decode
    /// kind:23195 responses before the kernel drops them as unknown kinds.
    ///
    /// MUST be called before any `nmp_app_wallet_*` FFI symbol is invoked;
    /// otherwise the wallet runtime is unreachable and the actions surface
    /// a `Failed` terminal. A poisoned mutex is a silent no-op (D6).
    pub fn set_relay_text_interceptor(
        &self,
        interceptor: std::sync::Arc<dyn nmp_core::substrate::RelayTextInterceptor>,
    ) {
        if let Ok(mut slot) = self.relay_text_interceptor.lock() {
            slot.clear();
            slot.push(interceptor);
        }
    }

    /// Add a relay-text interceptor without removing existing ones.
    ///
    /// Protocol runtimes installed by different composition layers can share
    /// the same inbound frame stream. Each runtime filters the frames it owns
    /// and returns an empty vector for the rest.
    pub fn add_relay_text_interceptor(
        &self,
        interceptor: std::sync::Arc<dyn nmp_core::substrate::RelayTextInterceptor>,
    ) {
        if let Ok(mut slot) = self.relay_text_interceptor.lock() {
            slot.push(interceptor);
        }
    }

    /// V-40 — register a [`nmp_core::substrate::IngestParser`] for `kind`
    /// against the shared `EventIngestDispatcher` slot. The same `Arc`
    /// the actor binds onto the kernel, so a registration is visible to
    /// the ingest path immediately (no actor round-trip needed).
    ///
    /// Per-NIP crates call this through their `register_actions` entry
    /// point (today: `nmp_nip17::register_actions` registers the
    /// `Kind10050Parser`). MUST be called before `nmp_app_start` so the
    /// kernel sees the parser when the first event of `kind` arrives.
    ///
    /// D6 — a poisoned dispatcher lock is a silent no-op (the
    /// registration is dropped; existing registrations are preserved).
    pub fn register_ingest_parser(
        &self,
        kind: u32,
        parser: std::sync::Arc<dyn nmp_core::substrate::IngestParser>,
    ) {
        if let Ok(mut d) = self.ingest_dispatcher_slot.write() {
            d.register_kind(kind, parser);
        }
    }

    /// V-40 — install the kernel's [`nmp_core::substrate::DmInboxRelayLookup`]
    /// handle. The per-app crate (today `nmp-nip17::register_actions`)
    /// hands in a concrete `DmRelayCache`; the same `Arc` is the writer
    /// side fed by the kind:10050 parser registered above + the reader
    /// side the kernel exposes through `recipient_dm_relays` and the
    /// planner-side `KernelMailboxes` adapter.
    ///
    /// MUST be called before `nmp_app_start` AND before any kind:10050
    /// event is ingested (the caches are independent stores; a late swap
    /// would lose entries written into the old cache).
    pub fn set_dm_inbox_relay_lookup(
        &self,
        lookup: std::sync::Arc<dyn nmp_core::substrate::DmInboxRelayLookup>,
    ) {
        if let Ok(mut slot) = self.dm_inbox_relays_slot.lock() {
            *slot = lookup;
        }
    }

    /// V-51 phase 5 — install the per-app substrate-routing factory.
    ///
    /// `factory` is a `Send + Sync` closure that, given the kernel's
    /// `RoutingTraceObserver` (the kernel-owned
    /// `Arc<RoutingTraceProjection>` clone), returns the
    /// `(Arc<dyn OutboxRouter>, Arc<dyn MailboxCache>)` pair the actor
    /// installs via [`crate::kernel::Kernel::set_routing`]. Production
    /// composition (`nmp-app-chirp`) writes a closure constructing
    /// `nmp_router::GenericOutboxRouter` (with the observer threaded
    /// through `with_trace_observer`) + `nmp_router::InMemoryMailboxCache`.
    ///
    /// MUST be called BEFORE `nmp_app_start` AND BEFORE any kind:10002
    /// event is ingested. The substrate caches the kernel holds and the
    /// production `nmp_router::InMemoryMailboxCache` are independent
    /// stores, not a write-through pair — a swap after ingest would lose
    /// the cached entries. `D6`: a poisoned slot is a silent no-op (the
    /// kernel keeps its in-crate defaults; the production swap is a
    /// no-op for that one process).
    ///
    /// The factory is re-invoked by the `Reset` dispatch arm against the
    /// rebuilt kernel's fresh projection so the production router/cache
    /// pair survives a state wipe (mirrors the `routing_trace_slot`
    /// re-publish step).
    pub fn set_routing_substrate<F>(&self, factory: F)
    where
        F: Fn(
                std::sync::Arc<dyn nmp_core::substrate::RoutingTraceObserver>,
            ) -> (
                std::sync::Arc<dyn nmp_core::substrate::OutboxRouter>,
                std::sync::Arc<dyn nmp_core::substrate::MailboxCache>,
            ) + Send
            + Sync
            + 'static,
    {
        if let Ok(mut slot) = self.routing_substrate.lock() {
            *slot = Some(std::sync::Arc::new(factory));
        }
    }

    /// Install the substrate-publish-resolver factory.
    ///
    /// Spec §271 (2026-05-25): `Nip65OutboxResolver` was moved out of
    /// `nmp-core::publish::nip65` into `nmp-router` (Layer 2). The kernel
    /// constructs `PublishEngine` with the in-crate `NoopOutboxResolver`
    /// default (fail-closed); production composition
    /// (`nmp-app-template::register_defaults`) installs the
    /// router-side resolver through this factory slot. The actor reads
    /// the slot right after kernel construction (and on `Reset`) and
    /// invokes [`crate::kernel::Kernel::set_publish_resolver`] with the
    /// produced `Arc<dyn OutboxResolver>`.
    ///
    /// `factory` receives the four kernel-owned handles the router-side
    /// `Nip65OutboxResolver` needs (`EventStore` + the indexer /
    /// local-write / active-account slots). Production composition writes
    /// a closure returning `Arc::new(Nip65OutboxResolver::with_local_relays(...))`
    /// over those handles, so the resolver reads through the same shared
    /// state the kernel actor writes to (D4 sole-writer preserved).
    ///
    /// MUST be called BEFORE `nmp_app_start` AND BEFORE any kind:10002
    /// event is ingested. `D6`: a poisoned slot is a silent no-op (the
    /// kernel keeps its `NoopOutboxResolver`; every publish then fails
    /// closed with `NoTargets`).
    ///
    /// The factory is re-invoked by the `Reset` dispatch arm against the
    /// rebuilt kernel's fresh handles so the production resolver survives
    /// a state wipe (mirrors `set_routing_substrate`).
    pub fn set_publish_resolver_factory<F>(&self, factory: F)
    where
        F: Fn(
                std::sync::Arc<dyn nmp_core::store::EventStore>,
                nmp_core::slots::IndexerRelaysSlot,
                nmp_core::slots::LocalWriteRelaysSlot,
                nmp_core::slots::ActiveAccountSlot,
            ) -> std::sync::Arc<dyn nmp_core::publish::OutboxResolver>
            + Send
            + Sync
            + 'static,
    {
        if let Ok(mut slot) = self.publish_resolver.lock() {
            *slot = Some(std::sync::Arc::new(factory));
        }
    }

    /// Test-only: run every registered snapshot projection directly against
    /// the app's shared registry, bypassing the actor/kernel tick. The
    /// end-to-end `make_update`-driven proof lives in the kernel test module
    /// (`kernel/snapshot_registry_tests.rs`); this helper lets the FFI
    /// registration tests assert the C-callback bridge in isolation.
    #[cfg(test)]
    pub(crate) fn run_snapshot_projections_for_test(
        &self,
    ) -> std::collections::HashMap<String, serde_json::Value> {
        self.snapshot_projections
            .lock()
            .map(|registry| registry.run())
            .unwrap_or_default()
    }

    /// Test-only direct execution path into the action registry.
    ///
    /// Bypasses [`ActionRegistry::start`] (which needs a
    /// registered *module* to validate the JSON shape) so a unit test can
    /// exercise a host-registered *executor* on its own — the v1 seam only
    /// exposes executor registration, not module registration. A fixed
    /// placeholder `correlation_id` stands in for the registry-minted id that
    /// the real `dispatch_action` path threads in.
    #[cfg(test)]
    pub(crate) fn test_execute_action(
        &self,
        namespace: &str,
        action_json: &str,
    ) -> Result<(), String> {
        self.action_registry
            .execute(namespace, action_json, "test-correlation-id", &|cmd| {
                self.send_cmd(cmd)
            })
    }

    pub(crate) fn set_pending_mls_autopublish(&self, enabled: bool) {
        self.pending_mls_autopublish
            .store(enabled, Ordering::Release);
    }

    /// Reads the one-shot MLS-autopublish intent and clears it in the same
    /// atomic step (`swap`), so a second caller cannot re-observe the flag.
    /// Atomics cannot poison, so — unlike the previous `Mutex<bool>` — there
    /// is no lock-failure fallback path that could silently drop the intent.
    #[must_use]
    pub fn take_pending_mls_autopublish(&self) -> bool {
        self.pending_mls_autopublish.swap(false, Ordering::AcqRel)
    }

    /// Clone of the actor command sender. Used by Rust-side runtime
    /// controllers that need to report work back to the actor without a C
    /// round-trip.
    ///
    /// G-S4 caveat: sends through this raw clone bypass the `queue_depth`
    /// straddle counter (`send_cmd` is the only incrementing path). The
    /// `actor_queue_depth` snapshot metric is therefore a lower bound when a
    /// broker is wired — acceptable for a backpressure gate that watches for
    /// buildup, not exact occupancy.
    #[must_use]
    pub fn actor_sender(&self) -> Sender<ActorCommand> {
        self.tx.clone()
    }

    /// Import a local secret through the actor-owned identity reducer.
    pub fn sign_in_nsec(&self, secret: Zeroizing<String>) {
        self.send_cmd(ActorCommand::SignInNsec { secret });
    }

    /// Restore an app-scoped local secret from the keyring capability or use
    /// an injected test secret, then sign it in through the identity reducer.
    pub fn restore_local_nsec_from_keyring(
        &self,
        account_id: &str,
        test_nsec: Option<String>,
    ) -> Option<String> {
        let secret = match test_nsec {
            Some(secret) => Some(secret),
            None => self.recall_local_nsec(account_id),
        }?;
        self.sign_in_nsec(Zeroizing::new(secret.clone()));
        Some(secret)
    }

    /// Persist a newly-imported local secret through the keyring capability,
    /// then sign it in through the identity reducer.
    #[must_use]
    pub fn sign_in_local_nsec_with_keyring(&self, account_id: &str, secret: String) -> String {
        let req = nmp_core::substrate::KeyringIdentityWiring::persist_secret(
            "nmp.identity.persist",
            account_id,
            &secret,
        );
        let _ = self.dispatch_capability(&req);
        self.sign_in_nsec(Zeroizing::new(secret.clone()));
        secret
    }

    /// Remove an identity through the actor-owned identity reducer.
    pub fn remove_account(&self, identity_id: String) {
        self.send_cmd(ActorCommand::RemoveAccount { identity_id });
    }

    /// Forget the app-scoped local secret and remove the identity through the
    /// actor-owned reducer.
    pub fn remove_account_forgetting_keyring(&self, account_id: &str, identity_id: String) {
        let req = nmp_core::substrate::KeyringIdentityWiring::forget_secret(
            "nmp.identity.forget",
            account_id,
        );
        let _ = self.dispatch_capability(&req);
        self.remove_account(identity_id);
    }

    fn recall_local_nsec(&self, account_id: &str) -> Option<String> {
        let req = nmp_core::substrate::KeyringIdentityWiring::recall_secret(
            "nmp.identity.recall",
            account_id,
        );
        let envelope = self.dispatch_capability(&req);
        let result = nmp_core::substrate::KeyringIdentityWiring::decode_result(&envelope);
        match result.status {
            nmp_core::substrate::KeyringStatus::Ok => result.secret,
            nmp_core::substrate::KeyringStatus::NotFound
            | nmp_core::substrate::KeyringStatus::Error => None,
        }
    }

    /// T146 — register a typed Rust observer. Returns an opaque id the
    /// caller retains to unregister later via
    /// [`Self::unregister_event_observer`]. Used by per-app crates such as
    /// `nmp-app-chirp` which depend on `nmp-core` + a protocol crate
    /// (`nmp-nip01`) and need typed `&KernelEvent` access on the kernel's
    /// ingest fan-out. D0 — `nmp-core` never names the protocol crate; this
    /// trait is the seam.
    #[must_use]
    pub fn register_event_observer(
        &self,
        observer: Arc<dyn KernelEventObserver>,
    ) -> KernelEventObserverId {
        register_rust_observer(&self.event_observers, observer)
    }

    /// T146 — unregister a previously-registered observer. Idempotent;
    /// unknown ids are silent no-ops (D6).
    pub fn unregister_event_observer(&self, id: KernelEventObserverId) {
        unregister_observer(&self.event_observers, id);
    }

    /// T146 — clone of the kernel event observer slot. The `ffi::event_observer`
    /// FFI surface uses this to plug C-ABI registrations into the same slot
    /// that backs the typed Rust API above. Crate-private because external
    /// Rust callers should go through
    /// [`Self::register_event_observer`] / [`Self::unregister_event_observer`].
    #[must_use]
    pub(crate) fn event_observers_slot(&self) -> KernelEventObserverSlot {
        Arc::clone(&self.event_observers)
    }

    /// Register a typed Rust raw signed-event observer with a kind filter
    /// (empty filter → all kinds). Returns an opaque id the caller retains
    /// to unregister via [`Self::unregister_raw_event_observer`]. Used by
    /// per-app / protocol crates that need the verbatim signed event
    /// (`sig` included) — e.g. an inbound-ingest seam that must hand the
    /// whole signed event to its own state machine. D0 — `nmp-core` never
    /// names the protocol; this trait is the generic seam.
    #[must_use]
    pub fn register_raw_event_observer(
        &self,
        kinds: KindFilter,
        observer: Arc<dyn RawEventObserver>,
    ) -> RawEventObserverId {
        register_rust_raw_observer(&self.raw_event_observers, kinds, observer)
    }

    /// Unregister a previously-registered raw observer. Idempotent;
    /// unknown ids are silent no-ops (D6).
    pub fn unregister_raw_event_observer(&self, id: RawEventObserverId) {
        unregister_raw_observer(&self.raw_event_observers, id);
    }

    /// Clone of the raw signed-event tap slot. The `ffi::raw_event_tap`
    /// FFI surface uses this to plug C-ABI registrations into the same
    /// slot that backs the typed Rust API above. Crate-private — external
    /// Rust callers go through [`Self::register_raw_event_observer`] /
    /// [`Self::unregister_raw_event_observer`].
    #[must_use]
    pub(crate) fn raw_event_observers_slot(&self) -> RawEventObserverSlot {
        Arc::clone(&self.raw_event_observers)
    }

    /// Atomically swap the per-app's NIP-17 DM-inbox raw-observer id slot:
    /// store `new` and return whatever was previously installed there.
    ///
    /// Used by per-app crates (e.g. `nmp-app-chirp`) to make their
    /// `register_dm_inbox` entry point idempotent across re-invokes (sign-in,
    /// account switch). Recipe:
    ///
    /// ```ignore
    /// let new_id = app.register_raw_event_observer(filter, projection);
    /// if new_id.0 == 0 { return; }
    /// if let Some(prev) = app.swap_nip17_dm_inbox_observer(Some(new_id)) {
    ///     app.unregister_raw_event_observer(prev);
    /// }
    /// ```
    ///
    /// The swap-then-unregister order is deliberate: storing the new id first
    /// means a host that aborts between register and unregister still has the
    /// new observer live (no inbox-gap window), and the take-and-set under a
    /// single lock acquisition makes the previous id impossible to lose to a
    /// concurrent re-invoke. A poisoned mutex degrades to `None` (D6).
    #[must_use]
    pub fn swap_nip17_dm_inbox_observer(
        &self,
        new: Option<RawEventObserverId>,
    ) -> Option<RawEventObserverId> {
        let mut guard = self.nip17_dm_inbox_observer_id.lock().ok()?;
        let prev = guard.take();
        *guard = new;
        prev
    }

    /// Atomically swap the per-app's singleton kernel-event observer-id slot:
    /// store `new` and return whatever was previously installed there.
    ///
    /// Mirrors [`Self::swap_nip17_dm_inbox_observer`] for the typed
    /// [`KernelEventObserver`] fan-out (in contrast to the raw signed-event
    /// tap). Same idempotent-re-invoke contract: a per-app crate that wires
    /// exactly one auxiliary `KernelEventObserver` per app uses this slot
    /// to ensure a second registration unregisters the first one before
    /// installing itself. A poisoned mutex degrades to `None` (D6).
    ///
    /// The slot is substrate-generic (D0 — the kernel never names the app
    /// noun); the per-app crate decides what protocol surface the singleton
    /// observer projects.
    #[must_use]
    pub fn swap_singleton_event_observer(
        &self,
        new: Option<KernelEventObserverId>,
    ) -> Option<KernelEventObserverId> {
        let mut guard = self.singleton_event_observer_id.lock().ok()?;
        let prev = guard.take();
        *guard = new;
        prev
    }

    /// Push a `LogicalInterest` into the subscription registry and schedule a
    /// recompile. Idempotent: same `InterestId` replaces the prior entry.
    ///
    /// Used by protocol crates (e.g. `nmp-marmot`) to register persistent
    /// relay subscriptions — kind:1059 `#p <pubkey>` for gift-wrap Welcome
    /// delivery, per-group kind:445 feeds, etc. The kernel emits REQ frames
    /// on the next compile pass; matching inbound events then flow through the
    /// raw-event tap automatically, with no Swift polling needed.
    pub fn push_interest(&self, interest: nmp_core::planner::LogicalInterest) {
        self.send_cmd(ActorCommand::PushInterest(interest));
    }

    /// Route a typed capability request through the registered native
    /// callback. Protocol/app composition crates use this when Rust owns the
    /// policy and native only executes the platform capability.
    #[must_use]
    pub fn dispatch_capability(
        &self,
        request: &nmp_core::substrate::CapabilityRequest,
    ) -> nmp_core::substrate::CapabilityEnvelope {
        let json = serde_json::to_string(request).unwrap_or_else(|_| "{}".to_string());
        let payload = dispatch_capability(&self.capability_callback, &json);
        serde_json::from_str(&payload).unwrap_or_else(|_| nmp_core::substrate::CapabilityEnvelope {
            namespace: request.namespace.clone(),
            correlation_id: request.correlation_id.clone(),
            result_json: r#"{"status":"error","os_status":-50}"#.to_string(),
        })
    }

    /// Return the active local (nsec-backed) secret key in `nsec1…` bech32
    /// form, or `None` when no local account is active (remote signer or no
    /// account). The actor writes this slot synchronously before emitting
    /// each identity-change snapshot, so callers inside `apply()` callbacks
    /// always see the up-to-date value. Used by per-app crates (e.g.
    /// per-app crate registration) so the key stays Rust-owned
    /// (D0 — Swift never sees it for the `createAccount` path).
    #[must_use]
    pub fn mls_local_nsec(&self) -> Option<Zeroizing<String>> {
        self.mls_local_nsec.lock().ok()?.clone()
    }

    /// Clone of the active-local-`nostr::Keys` slot — substrate-generic.
    ///
    /// Returns a clone of the shared `Arc` so the caller (e.g. a
    /// `DmInboxProjection` for NIP-17 gift-wrap unsealing, or a
    /// `ZapReceiptsRuntimeController` reading the active pubkey for the
    /// NIP-57 self-zap subscription) holds its own handle and reads the
    /// current keys on demand. The actor is the sole writer; it updates
    /// the inner `Option<Keys>` on every identity mutation, so a long-lived
    /// consumer always observes the up-to-date account without
    /// re-registering.
    ///
    /// This is DELIBERATELY separate from [`Self::mls_local_nsec`]: that
    /// accessor backs the ADR-0025 Marmot exception (raw bech32 nsec,
    /// D13-policed); this one is the substrate-generic active-keys feed.
    #[must_use]
    pub fn active_local_keys(&self) -> ActiveLocalKeysSlot {
        Arc::clone(&self.active_local_keys)
    }

    /// V-51 phase 4 — clone of the kernel's [`RoutingTraceProjection`]
    /// (`Arc`).
    ///
    /// Returns `None` until the actor has constructed the kernel
    /// (the first command after `nmp_app_new` causes the actor to build
    /// the kernel; the projection is published into the slot immediately
    /// after — see `run_actor_with_observers`). Once `Some`, the same
    /// projection survives until `Reset`, which rebuilds the kernel and
    /// re-publishes a fresh projection clone into the slot.
    ///
    /// Per-app crates (chirp-repl `routing-trace` subcommand, the
    /// `nmp-testing` validation harness) read recent routing decisions via
    /// [`crate::RoutingTraceProjection::snapshot_publishes`] /
    /// `snapshot_subscriptions` on the returned `Arc`. The projection is
    /// the consumer side of the V-51 substrate `RoutingTraceObserver` seam;
    /// the kernel hands the projection to the production router
    /// (`nmp_router::GenericOutboxRouter::with_trace_observer`) via the
    /// `RoutingSubstrateSlot` factory invoked at actor-startup time.
    /// The default `EmptyOutboxRouter` never fires the observer, so a
    /// session that never installs a real router will see an empty
    /// projection (substrate-honest debt B, 2026-05-24).
    #[must_use]
    pub fn routing_trace(&self) -> Option<Arc<nmp_core::RoutingTraceProjection>> {
        self.routing_trace.lock().ok()?.clone()
    }

    /// Clone of the live relay-edit row slot.
    ///
    /// Per-app Rust controllers use this to derive protocol-specific relay
    /// projections without asking platform shells to parse `RelayEditRow.role`.
    /// The actor is the sole writer; callers should take quick snapshots only.
    ///
    /// The slot type is [`nmp_core::RelayEditRowsSlot`] — a
    /// newtype `Arc<Mutex<RelayEditRowList>>`. Readers iterate via
    /// `guard.as_slice()` so they never touch the inner `Vec` directly. D14
    /// (`crates/nmp-testing/bin/doctrine-lint/rules/d14.rs`) forbids new bare
    /// `Arc<Mutex<Vec<…>>>` fields on `NmpApp`; the typed alias makes the
    /// slot's purpose visible at every call site.
    #[must_use]
    pub fn relay_edit_rows_handle(&self) -> nmp_core::RelayEditRowsSlot {
        Arc::clone(&self.relay_edit_rows)
    }

    /// Return the user's current write-relay URLs, read from the shared kernel relay-edit
    /// projection. Empty when the user has not configured any write relays.
    /// Used by per-app crates so relay resolution stays Rust-owned (D0).
    ///
    /// The underlying slot is a typed `RelayEditRowList`; the
    /// reader iterates via `as_slice()` so it never touches the inner `Vec`
    /// directly.
    #[must_use]
    pub fn write_relay_urls(&self) -> Vec<String> {
        let Ok(guard) = self.relay_edit_rows.lock() else {
            return Vec::new();
        };
        guard
            .as_slice()
            .iter()
            .filter(|r| has_role(r.role(), "write"))
            .map(|r| r.url().to_string())
            .collect()
    }

    /// Workspace-internal kernel publish API — verbatim publish of an
    /// already-signed `nostr::Event` to an EXPLICIT relay set. Empty or
    /// malformed relay sets fail closed in the actor publish handler; callers
    /// that want `Auto` routing must use the typed `nmp.publish` action path
    /// with `PublishTarget::Auto`.
    ///
    /// One door per capability — this is the Rust-typed replacement for
    /// the deleted `nmp_app_publish_signed_event*` `extern "C"` symbols. App
    /// composition crates that retain an `NmpApp` (e.g. `nmp-marmot`'s
    /// `MarmotProjection`) reach the kernel through this method instead of
    /// re-declaring those symbols in their own `extern "C"` blocks. The
    /// Schnorr signature + event-id hash are verified on the actor side
    /// (same `commands::publish::publish_signed_event` path the deleted FFI
    /// symbols used to land on); forged or garbled events are dropped with a
    /// kernel toast.
    ///
    /// Routing is fail-closed: this entrypoint always builds a
    /// `PublishTarget::Explicit { relays }`, bypassing the outbox resolver.
    /// Marmot uses this for relay-pinned kind:445 commits / messages and as
    /// the documented kind:1059 inbox-routing approximation. Callers that
    /// want NIP-65 outbox (`PublishTarget::Auto`) must use the typed
    /// `nmp.publish` action path through `dispatch_action` so `Auto` and
    /// `Explicit` never share the same empty-vector encoding.
    ///
    /// kind:1059 envelopes additionally hit the kernel-side D10 defensive
    /// guard in `commands::publish::publish_signed_event`: it refuses any
    /// kind:1059 envelope whose `relays` slice is empty, sets a D6 toast on
    /// the kernel, and drops the envelope — the same behaviour the
    /// call-site guard in `nmp_nip17::SendGiftWrappedDmCommand` gives the
    /// NIP-17 send path (V-39 moved the orchestration out of nmp-core).
    /// The Marmot bridge's own runtime guard in
    /// `nmp-marmot::projection::publish::publish_to` is the matching guard
    /// for the C-ABI symbol path; together they make a kind:1059 Auto-route
    /// structurally impossible regardless of which entry point a caller
    /// reaches the kernel through.
    ///
    /// Theme A discriminator (see `substrate/action.rs`): this is the
    /// system-authored / lifecycle exception to "every event-producing
    /// publish goes through `dispatch_action`". Marmot publishes MLS-signed
    /// events whose outer signature was minted by an ephemeral key (gift
    /// wraps) or by an MLS group credential — neither of which the kernel's
    /// signer can re-mint. The generic action seam (`nmp.publish`) signs +
    /// publishes; this entrypoint publishes verbatim without re-signing.
    ///
    /// Fire-and-forget (D6): a poisoned actor channel is a silent drop, the
    /// same as the deleted FFI symbols. `correlation_id` is always `None`
    /// here — this path is not the `dispatch_action` action-result channel.
    pub fn publish_signed_explicit(&self, event: nostr::Event, relays: &[nostr::RelayUrl]) {
        // RawEvent (flat NIP-01) is what `ActorCommand::PublishSignedEvent`
        // carries; `commands::publish::publish_signed_event` runs the
        // `VerifiedEvent::try_from_raw` gate (signature + id hash) before
        // anything else, so a Marmot caller that constructed `event` from a
        // dispatch op's signed-JSON output is still subject to the same
        // crypto bar as a wire-arrived event. The `tags` clone mirrors
        // every other RawEvent construction site in the crate
        // (`commands::publish` action_registry.rs:420).
        let raw = nmp_core::store::RawEvent {
            id: event.id.to_hex(),
            pubkey: event.pubkey.to_hex(),
            created_at: event.created_at.as_secs(),
            kind: u32::from(event.kind.as_u16()),
            tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
            content: event.content.clone(),
            sig: event.sig.to_string(),
        };
        let relays: Vec<nmp_core::publish::RelayUrl> = relays
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        self.send_cmd(ActorCommand::PublishSignedEvent {
            raw,
            target: nmp_core::publish::PublishTarget::Explicit { relays },
            correlation_id: None,
        });
    }

    /// Choose the relay for a client-initiated NIP-46 `nostrconnect://`
    /// handshake from the shared kernel relay-edit projection. Empty or
    /// poisoned state falls back through the same Rust-owned policy as an app
    /// with no configured write relays.
    #[must_use]
    pub fn nostrconnect_relay_url(&self) -> String {
        let Ok(guard) = self.relay_edit_rows.lock() else {
            return nmp_core::NOSTRCONNECT_DEFAULT_RELAY_URL.to_string();
        };
        // Typed slot — iterate via `as_slice()` so the inner `Vec`
        // never leaks through this consumer.
        nostrconnect_relay_url(guard.as_slice().iter().map(|row| (row.url(), row.role())))
    }
}

impl nmp_core::substrate::ActionRegistrar for NmpApp {
    fn register_action<M: nmp_core::substrate::ActionModule + 'static>(&mut self) {
        NmpApp::register_action::<M>(self);
    }
}

impl nmp_core::substrate::AppHost for NmpApp {
    fn register_snapshot_projection<K, F>(&self, key: K, f: F)
    where
        K: Into<String>,
        F: Fn() -> serde_json::Value + Send + Sync + 'static,
    {
        NmpApp::register_snapshot_projection(self, key, f);
    }

    fn set_coverage_hook(&self, hook: nmp_core::subs::PlanCoverageHook) {
        NmpApp::set_coverage_hook(self, hook);
    }

    fn set_req_frame_interceptor(
        &self,
        interceptor: Arc<dyn nmp_core::substrate::ReqFrameInterceptor>,
    ) {
        NmpApp::set_req_frame_interceptor(self, interceptor);
    }

    fn add_relay_text_interceptor(
        &self,
        interceptor: Arc<dyn nmp_core::substrate::RelayTextInterceptor>,
    ) {
        NmpApp::add_relay_text_interceptor(self, interceptor);
    }

    fn register_ingest_parser(
        &self,
        kind: u32,
        parser: Arc<dyn nmp_core::substrate::IngestParser>,
    ) {
        NmpApp::register_ingest_parser(self, kind, parser);
    }

    fn set_dm_inbox_relay_lookup(
        &self,
        lookup: Arc<dyn nmp_core::substrate::DmInboxRelayLookup>,
    ) {
        NmpApp::set_dm_inbox_relay_lookup(self, lookup);
    }

    fn set_routing_substrate<F>(&self, factory: F)
    where
        F: Fn(
                Arc<dyn nmp_core::substrate::RoutingTraceObserver>,
            ) -> (
                Arc<dyn nmp_core::substrate::OutboxRouter>,
                Arc<dyn nmp_core::substrate::MailboxCache>,
            ) + Send
            + Sync
            + 'static,
    {
        NmpApp::set_routing_substrate(self, factory);
    }

    fn set_publish_resolver_factory<F>(&self, factory: F)
    where
        F: Fn(
                Arc<dyn nmp_core::store::EventStore>,
                nmp_core::slots::IndexerRelaysSlot,
                nmp_core::slots::LocalWriteRelaysSlot,
                nmp_core::slots::ActiveAccountSlot,
            ) -> Arc<dyn nmp_core::publish::OutboxResolver>
            + Send
            + Sync
            + 'static,
    {
        NmpApp::set_publish_resolver_factory(self, factory);
    }

    fn active_local_keys(&self) -> nmp_core::slots::ActiveLocalKeysSlot {
        NmpApp::active_local_keys(self)
    }

    fn actor_sender(&self) -> Sender<ActorCommand> {
        NmpApp::actor_sender(self)
    }

    fn register_event_observer(
        &self,
        observer: Arc<dyn KernelEventObserver>,
    ) -> KernelEventObserverId {
        NmpApp::register_event_observer(self, observer)
    }

    fn unregister_event_observer(&self, id: KernelEventObserverId) {
        NmpApp::unregister_event_observer(self, id);
    }

    fn swap_singleton_event_observer(
        &self,
        new: Option<KernelEventObserverId>,
    ) -> Option<KernelEventObserverId> {
        NmpApp::swap_singleton_event_observer(self, new)
    }

    fn register_raw_event_observer(
        &self,
        kinds: KindFilter,
        observer: Arc<dyn RawEventObserver>,
    ) -> RawEventObserverId {
        NmpApp::register_raw_event_observer(self, kinds, observer)
    }

    fn unregister_raw_event_observer(&self, id: RawEventObserverId) {
        NmpApp::unregister_raw_event_observer(self, id);
    }

    fn swap_nip17_dm_inbox_observer(
        &self,
        new: Option<RawEventObserverId>,
    ) -> Option<RawEventObserverId> {
        NmpApp::swap_nip17_dm_inbox_observer(self, new)
    }

    fn relay_edit_rows_handle(&self) -> nmp_core::RelayEditRowsSlot {
        NmpApp::relay_edit_rows_handle(self)
    }
}

// SAFETY: `app` is a raw pointer from `nmp_app_new()`. The function is `extern "C"` (callable
// from Swift/C) so it cannot be marked `unsafe` at the Rust level; the caller guarantees the
// pointer contract. The `allow` suppresses the clippy::not_unsafe_ptr_arg_deref lint which
// does not distinguish between `extern "C"` FFI boundaries and ordinary Rust functions.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_free(app: *mut NmpApp) {
    if !app.is_null() {
        // SAFETY: caller guarantees app is a valid pointer allocated by nmp_app_new().
        unsafe {
            drop(Box::from_raw(app));
        }
    }
}

#[no_mangle]
pub extern "C" fn nmp_app_set_update_callback(
    app: *mut NmpApp,
    context: *mut c_void,
    callback: Option<UpdateCallback>,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Ok(mut slot) = app.update_callback.lock() else {
        return;
    };
    *slot = callback.map(|callback| UpdateCallbackRegistration {
        context: context as usize,
        callback,
    });
}

/// Set the persistent storage directory for the LMDB `EventStore` backend.
///
/// Threads the host-supplied path through to the kernel so the
/// `lmdb-backend` feature can be used in production (iOS / Android). When
/// the crate is built without `--features lmdb-backend` the path is stored
/// but inert — the in-memory store is always used.
///
/// Call ordering: this MUST be called before [`nmp_app_start`]. The kernel
/// resolves its `EventStore` once, on the actor thread, when the first
/// `Start` would otherwise need it; a path set after the kernel is built
/// has no effect until the next process launch. A `NULL` or empty `path`
/// clears any previously-set path (the kernel then falls back to the
/// `NMP_LMDB_PATH` env var, or the in-memory store).
///
/// Mirrors the `app_ref` + `Mutex::lock` pattern of the other
/// `nmp_app_set_*` setters — no panic can cross the C ABI boundary because
/// the body performs no foreign callback and no panicking operation.
///
/// # Safety
/// `app` must be a valid non-null pointer from [`nmp_app_new`], or null
/// (a null `app` is a silent no-op). `path` must be a valid UTF-8
/// null-terminated C string, or null. Invalid UTF-8 is treated as "unset".
#[no_mangle]
pub extern "C" fn nmp_app_set_storage_path(app: *mut NmpApp, path: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    // `c_optional_string_argument` collapses NULL / empty / whitespace to
    // `None` and returns `Some(trimmed)` otherwise — exactly the
    // "empty clears, non-empty sets" semantics documented above. It also
    // rejects invalid UTF-8 (→ `None`), so no panic is possible here.
    let resolved = c_optional_string_argument(path);
    let Ok(mut slot) = app.storage_path.lock() else {
        return;
    };
    *slot = resolved;
}

#[no_mangle]
pub extern "C" fn nmp_app_start(
    app: *mut NmpApp,
    _events_per_second: c_uint,
    visible_limit: c_uint,
    emit_hz: c_uint,
) {
    let Some(app) = app_ref(app) else {
        return;
    };

    app.send_cmd(ActorCommand::Start {
        visible_limit: clamp_visible(visible_limit),
        emit_hz: clamp_emit_hz(emit_hz),
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_configure(
    app: *mut NmpApp,
    _events_per_second: c_uint,
    visible_limit: c_uint,
    emit_hz: c_uint,
) {
    let Some(app) = app_ref(app) else {
        return;
    };

    app.send_cmd(ActorCommand::Configure {
        visible_limit: clamp_visible(visible_limit),
        emit_hz: clamp_emit_hz(emit_hz),
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_stop(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    app.send_cmd(ActorCommand::Stop);
}

#[no_mangle]
pub extern "C" fn nmp_app_reset(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    app.send_cmd(ActorCommand::Reset);
}

#[must_use]
pub(crate) fn app_ref<'a>(app: *mut NmpApp) -> Option<&'a NmpApp> {
    if app.is_null() {
        None
    } else {
        // SAFETY: caller guarantees non-null app is a valid NmpApp pointer.
        Some(unsafe { &*app })
    }
}

// ADR-0027 deleted `app_ref_mut`. Its only callers were the C-ABI
// `nmp_app_register_action_executor` / `nmp_app_register_action_module`
// registration symbols, which were themselves deleted as part of collapsing
// the dual-seam closure path. The typed registration seam
// (`NmpApp::register_action::<M>`) is Rust-only and takes `&mut self`
// directly; no C-ABI counterpart exists, so no `*mut NmpApp` → `&mut NmpApp`
// helper is needed.

#[must_use]
pub(crate) fn c_string_argument(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }

    // SAFETY: caller guarantees ptr is a valid null-terminated C string.
    // Validation: to_str() will reject invalid UTF-8.
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

/// Optional-string FFI argument. Unlike `c_string_argument` (which collapses
/// NULL / empty / whitespace to `None` for a REQUIRED arg and the caller
/// drops the call), this returns `Some(value)` for non-empty content and
/// `None` for absent — so a NULL `reply_to_id` means "top-level note" rather
/// than "drop the publish". Build-doc §1.1 contract.
#[must_use]
pub(crate) fn c_optional_string_argument(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller guarantees ptr is a valid null-terminated C string.
    let value = unsafe { CStr::from_ptr(ptr) }.to_str().ok()?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

fn clamp_visible(visible_limit: c_uint) -> usize {
    if visible_limit == 0 {
        DEFAULT_VISIBLE_LIMIT
    } else {
        visible_limit.clamp(1, 500) as usize
    }
}

fn clamp_emit_hz(emit_hz: c_uint) -> u32 {
    if emit_hz == 0 {
        DEFAULT_EMIT_HZ
    } else {
        emit_hz.clamp(1, 12)
    }
}
