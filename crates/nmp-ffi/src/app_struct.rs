//! `NmpApp` struct definition + pre-construction type aliases.
//!
//! Extracted from `lib.rs` to keep each file under the 500-LOC ceiling
//! (AGENTS.md file-size rule). No logic lives here — pure data-structure
//! declarations and slot-constructor helpers.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use nmp_core::__ffi_internal::{
    ActionRegistry, CapabilityCallbackSlot, KernelEventObserverSlot, LifecycleObserverSlot,
    SnapshotProjectionSlot,
};
use nmp_core::slots::{
    ActiveAccountSlot, ActiveLocalKeysSlot, EventStoreSlot, ExternalEventSinkPolicySlot,
    MlsLocalNsecSlot, NostrConnectBootstrapRelaySlot, NostrConnectPermsSlot,
    PublishResolverSlot, PullCursorRegistryHandleSlot, RoutingSubstrateSlot, RoutingTraceSlot,
    SingletonEventObserverIdSlot, StoragePathSlot,
};
use nmp_core::subs::PlanCoverageHook;
use nmp_core::KernelEventObserverId;
use crate::passive_start::ActorStarter;
use std::ffi::c_void;
use std::sync::mpsc;
use zeroize::Zeroizing;

// ── Update-callback types ─────────────────────────────────────────────────

/// C-ABI update callback signature — every `*const u8` / `usize` pair is a
/// FlatBuffers envelope byte slice borrowed for the callback's duration.
pub(crate) type UpdateCallback = extern "C" fn(*mut c_void, *const u8, usize);

/// One registered `(callback, context)` pair. `context` is a host-supplied
/// opaque pointer stored as `usize` to be `Send`; cast back to `*mut c_void`
/// only at call time (on the listener thread, under no Rust lock).
#[derive(Clone, Copy)]
pub(crate) struct UpdateCallbackRegistration {
    pub(crate) context: usize,
    pub(crate) callback: UpdateCallback,
}

// ── Update-callback quiescence gate ──────────────────────────────────────────

/// Inner mutable state for the update-callback quiescence gate.
///
/// Invariant: `in_flight > 0` only while the listener thread is actively
/// executing a foreign callback. The listener increments `in_flight` while
/// holding the mutex (so a concurrent `set_update_callback` cannot observe
/// `in_flight == 0` and return while a callback is still running), then
/// releases the mutex before invoking the foreign code. When the callback
/// returns the listener re-acquires the mutex, decrements `in_flight`, and
/// notifies `UpdateCallbackGate::drained` — which wakes any
/// `set_update_callback` call that is waiting for the old registration to
/// quiesce.
pub(crate) struct UpdateCallbackGateInner {
    pub(crate) registration: Option<UpdateCallbackRegistration>,
    pub(crate) in_flight: u32,
}

/// Quiescence-safe slot for the C-ABI update-callback registration.
///
/// Provides the contract: after [`nmp_app_set_update_callback`] returns, the
/// previously-registered `(callback, context)` pair is guaranteed to be
/// neither registered nor mid-invocation. Hosts may safely free the context
/// allocation once the setter returns.
///
/// **Design (option b — Condvar drain, no foreign code under lock):**
/// The listener thread increments `inner.in_flight` while holding the mutex,
/// drops the lock, calls the foreign function, then re-acquires the mutex to
/// decrement `in_flight` and signal `drained`. The setter waits on `drained`
/// until `in_flight` reaches zero before returning. This keeps all foreign
/// callback invocations outside any Rust mutex, avoiding deadlock even if a
/// host callback re-enters another NMP FFI entry point — the only
/// re-entrancy hazard that option (a) (invoke-under-lock) cannot tolerate.
///
/// **Re-entrancy note:** a host callback MUST NOT call
/// `nmp_app_set_update_callback` from within the callback itself — that
/// would deadlock because the setter waits for `in_flight` to reach zero,
/// which cannot happen while the callback is blocking.  No existing host
/// callback (iOS `nmpUpdateCallback`, Android `on_update`) calls back into
/// the setter, so this is not a live concern; it is documented here so
/// future implementors know the invariant.
///
/// Lives in this crate (not `nmp-core::slots`) because `UpdateCallback` is
/// a C-ABI function pointer type — a structurally-FFI surface concern.
pub(crate) struct UpdateCallbackGate {
    pub(crate) inner: Mutex<UpdateCallbackGateInner>,
    pub(crate) drained: std::sync::Condvar,
}

impl UpdateCallbackGate {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(UpdateCallbackGateInner {
                registration: None,
                in_flight: 0,
            }),
            drained: std::sync::Condvar::new(),
        }
    }
}

/// Typed slot for the C-ABI update callback registration.
///
/// Written by [`nmp_app_set_update_callback`]; read by the actor thread's
/// update-listener closure. Module-private: `UpdateCallbackRegistration` is
/// also module-private so the alias cannot be wider. Lives in this crate
/// (not `nmp-core::slots`) because the `UpdateCallback` C-ABI function
/// pointer type is a structurally-FFI shape; the actor reads through this
/// slot but the type itself is a C-ABI surface concern. The byte pointer is
/// borrowed only for the callback duration; hosts must copy before returning.
pub(crate) type UpdateCallbackSlot = Arc<UpdateCallbackGate>;

pub(crate) fn new_update_callback_slot() -> UpdateCallbackSlot {
    Arc::new(UpdateCallbackGate::new())
}

pub(crate) type IdentityChangeCallback = Arc<dyn Fn(Option<String>) + Send + Sync>;
pub(crate) type IdentityChangeObserverSlot = Arc<Mutex<Vec<IdentityChangeCallback>>>;

pub(crate) fn new_identity_change_observer_slot() -> IdentityChangeObserverSlot {
    Arc::new(Mutex::new(Vec::new()))
}

/// Host-installed preferred-relay source (the substrate-generic
/// [`PreferredRelaySource`](nmp_core::substrate::PreferredRelaySource) seam —
/// the kind:10007 read + app-default fallback for NIP-50 search). Populated by
/// the composition root through [`NmpApp::install_preferred_relay_source`] (the
/// `HostCapabilities` override); read by `open_search` to resolve
/// `UserPreferred` / `AppDefault` targets. `None` (the default) means no source
/// was installed, so those targets resolve to an empty relay set (cache-only
/// search, D6 graceful degrade). Not shared with the actor thread.
pub(crate) type SearchRelaySourceSlot =
    Arc<Mutex<Option<Arc<dyn nmp_core::substrate::PreferredRelaySource>>>>;

pub(crate) fn new_search_relay_source_slot() -> SearchRelaySourceSlot {
    Arc::new(Mutex::new(None))
}

pub(crate) fn read_active_account(slot: &ActiveAccountSlot) -> Option<String> {
    slot.lock().ok().and_then(|guard| guard.clone())
}

pub(crate) fn notify_identity_change_observers(
    active_account: &ActiveAccountSlot,
    last_notified: &Arc<Mutex<Option<String>>>,
    observers: &IdentityChangeObserverSlot,
) {
    let current = read_active_account(active_account);
    {
        let Ok(mut last) = last_notified.lock() else {
            return;
        };
        if *last == current {
            return;
        }
        *last = current.clone();
    }

    let callbacks = observers
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    for callback in callbacks {
        let current = current.clone();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            callback(current);
        }));
    }
}

// ── NmpApp struct ─────────────────────────────────────────────────────────────

pub struct NmpApp {
    pub(crate) tx: nmp_core::CommandSender,
    pub(crate) update_callback: UpdateCallbackSlot,
    /// Rust-side active-account observer registry. The update listener fires
    /// callbacks after the actor has written `active_account_handle` and before
    /// it forwards the same update frame to the native callback.
    pub(crate) identity_change_observers: IdentityChangeObserverSlot,
    pub(crate) capability_callback: CapabilityCallbackSlot,
    /// T118 / G3 — lifecycle observer slot. Shared `Arc` with the actor
    /// thread: registrations through [`lifecycle::nmp_app_set_lifecycle_callback`]
    /// are visible to the actor without crossing the FFI on each event.
    pub(crate) lifecycle_observer: LifecycleObserverSlot,
    /// T146 — kernel event observer slot. Shared `Arc` with the actor
    /// thread (and thus the kernel, which `crate::actor::run_actor_with_
    /// observers` binds onto the kernel via `set_event_observers_handle`).
    /// Per-app crates (e.g. a per-app crate) reach this slot through
    /// [`NmpApp::register_event_observer`] /
    /// [`NmpApp::unregister_event_observer`]; the C-ABI variant goes
    /// through `ffi::event_observer::nmp_app_register_event_observer`. Both
    /// paths mutate the same `Mutex<…>` the actor reads.
    pub(crate) event_observers: KernelEventObserverSlot,
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
    pub(crate) singleton_event_observer_id: SingletonEventObserverIdSlot,
    /// Shared relay-edit rows handle. Cloned to the actor thread and bound
    /// onto the kernel so external Rust callers (e.g. per-app crates) can read
    /// the user's current relay list without crossing FFI.
    ///
    /// The slot is a typed [`nmp_core::AppRelaySlot`]
    /// (`Arc<Mutex<AppRelayList>>`) — D14 forbids new bare
    /// `Arc<Mutex<Vec<…>>>` fields on `NmpApp` and the typed wrapper makes
    /// the slot's purpose visible at the declaration site.
    pub(crate) configured_relays: nmp_core::AppRelaySlot,
    /// Pre-start initial relay configuration. Set by `NmpAppBuilder::start()`
    /// (Rust path) or by calling `set_initial_relays_for_start` before
    /// `nmp_app_start`. The `nmp_app_start` function reads this slot and carries
    /// it in `ActorCommand::Start { initial_relays }`.
    ///
    /// D14: `Arc<Mutex<Vec<(String, String)>>>` is NOT the banned
    /// `Arc<Mutex<Vec<RowType>>>` projection shape — it is a one-shot pre-start
    /// staging slot consumed once by `nmp_app_start`, never shared with the
    /// actor thread as a live projection.
    pub(crate) initial_relays_for_start: Mutex<Vec<(String, String)>>,
    /// V-65 — host-supplied bootstrap relay URL for client-initiated NIP-46
    /// `nostrconnect://` handshakes when the user has no configured write relay.
    pub(crate) nostrconnect_bootstrap_relay: NostrConnectBootstrapRelaySlot,
    /// #1493 P9 — host-supplied NIP-46 permission request for client-initiated
    /// `nostrconnect://` handshakes.
    pub(crate) nostrconnect_perms: NostrConnectPermsSlot,
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
    pub(crate) mls_local_nsec: MlsLocalNsecSlot,
    /// Active account's local `nostr::Keys`, or `None` for a remote-signer
    /// (NIP-46 / bunker) account. The actor thread writes this after every
    /// identity mutation that changes the active local key.
    pub(crate) active_local_keys: ActiveLocalKeysSlot,
    /// V-82 — the active account's raw hex pubkey, or `None` when no account
    /// is signed in. This is the SAME `Arc` the kernel actor writes on every
    /// identity mutation.
    pub(crate) active_account_handle: ActiveAccountSlot,
    /// V-83 — the kernel's `EventStore` handle, published back by the actor
    /// right after kernel construction (and re-published on `Reset`).
    pub(crate) event_store_handle: EventStoreSlot,
    /// ADR-0058 step 3b — the kernel's pull-cursor registry handle, published
    /// back by the actor right after kernel construction (and re-published on
    /// `Reset`).
    pub(crate) pull_cursor_registry: PullCursorRegistryHandleSlot,
    /// FFI-supplied persistent storage directory for the LMDB `EventStore`
    /// backend. Set by [`nmp_app_set_storage_path`] before
    /// [`nmp_app_start`].
    pub(crate) storage_path: StoragePathSlot,
    /// V-51 phase 4 — slot the actor publishes the kernel's
    /// `RoutingTraceProjection` clone into right after kernel construction.
    pub(crate) routing_trace: RoutingTraceSlot,
    /// V-51 phase 5 — per-app substrate-routing factory slot.
    pub(crate) routing_substrate: RoutingSubstrateSlot,
    /// Spec §271 (2026-05-25) — per-app substrate-publish-resolver
    /// factory slot.
    pub(crate) publish_resolver: PublishResolverSlot,
    /// Test-support kernel-clock injection slot.
    pub(crate) kernel_clock: nmp_core::slots::KernelClockSlot,
    /// External event sink policy factory slot.
    pub(crate) external_event_sink_policy: ExternalEventSinkPolicySlot,
    /// One-shot account-creation intent: when true, the app-level MLS
    /// composition layer should publish a key package after it registers the new
    /// local identity.
    pub(crate) pending_mls_autopublish: AtomicBool,
    /// One-shot native actor starter. `nmp_app_start` consumes it, so
    /// pre-start commands queue without constructing the kernel.
    pub(crate) actor_starter: Mutex<Option<ActorStarter>>,
    /// Passive-handle bootstrap frame sender, dropped at start/drop.
    pub(crate) startup_update_tx: Mutex<Option<mpsc::Sender<nmp_core::UpdateFrameBytes>>>,
    pub(crate) actor: Mutex<Option<JoinHandle<()>>>,
    pub(crate) update_listener: Mutex<Option<JoinHandle<()>>>,
    /// M6 — namespace-keyed action-dispatch registry backing
    /// [`action::nmp_app_dispatch_action_bytes`].
    pub(crate) action_registry: ActionRegistry,
    /// ADR-0049 Part 2 — the composition ledger.
    pub(crate) composition_ledger: Arc<nmp_core::CompositionLedger>,
    /// ADR-0049 Part 2 — flips to `true` the first time `nmp_app_start` sends
    /// `ActorCommand::Start`.
    pub(crate) started: AtomicBool,
    /// Host-extensible typed snapshot output registry.
    pub(crate) snapshot_projections: SnapshotProjectionSlot,
    /// Reusable feed-controller registry.
    pub(crate) feed_registry: nmp_feed::FeedRegistrySlot,
    /// #1740 step 2 — the feed-SESSION registry.
    pub(crate) feed_sessions: Arc<nmp_feed::FeedSessionRegistry>,
    /// #1740 step 4 — app-registered custom-perspective definitions.
    pub(crate) custom_perspectives: Arc<nmp_feed::PerspectiveRegistry>,
    /// Per-open feed → ingest-observer bookkeeping for *transient* feeds.
    pub(crate) interest_feed_observers: Mutex<HashMap<String, KernelEventObserverId>>,
    /// G-S4 — straddle counter for the actor command channel depth.
    pub(crate) queue_depth: Arc<AtomicU64>,
    /// Test-only monotone send counter — counts every `send_cmd` call since
    /// construction, **never decremented**.
    #[cfg(test)]
    pub(crate) send_cmd_count: AtomicU64,
    /// Test-only last-command-variant tag — records the discriminant name of the
    /// most recently sent `ActorCommand` as a `'static str`. Unlike
    /// `send_cmd_count` (which only proves *something* was sent), this lets a
    /// test assert that the SPECIFIC variant that was expected was actually
    /// enqueued (e.g. `CancelPublish` vs. `RetryPublish`).
    ///
    /// Only compiled in `#[cfg(test)]`; zero overhead in production builds.
    #[cfg(test)]
    pub(crate) last_cmd_tag: std::sync::Mutex<Option<&'static str>>,
    /// D2 coverage-gate hook slot.
    pub(crate) coverage_hook: Arc<Mutex<Option<PlanCoverageHook>>>,
    /// Outbound planner REQ interceptor slot.
    pub(crate) req_frame_interceptor: nmp_core::substrate::ReqFrameInterceptorSlot,
    /// Host-installed host-op handler slot.
    pub(crate) host_op_handler: nmp_core::substrate::HostOpHandlerSlot,
    /// V-38: substrate-generic relay-text interceptor slot.
    pub(crate) relay_text_interceptor: nmp_core::substrate::RelayTextInterceptorSlot,
    /// ADR-0051 — relay-connected hook slot.
    pub(crate) relay_connected_hook: nmp_core::substrate::RelayConnectedHookSlot,
    /// ADR-0052 §D3 — per-app bunker-URI hook slot.
    pub(crate) bunker_hook: nmp_core::BunkerHookSlot,
    /// ADR-0052 §D3 — per-app NIP-55 restore hook slot.
    pub(crate) external_signer_hook: nmp_core::ExternalSignerHookSlot,
    /// ADR-0052 §D3 — per-app NIP-46 broker handle.
    #[cfg(feature = "signer-broker")]
    pub(crate) signer_broker: Arc<Mutex<Option<Arc<nmp_signer_broker::BunkerBroker>>>>,
    /// ADR-0052 §D3 — per-app NIP-55 driver handle.
    #[cfg(feature = "external-signer")]
    pub(crate) external_signer_driver:
        Arc<Mutex<Option<Arc<crate::external_signer::Nip55Driver>>>>,
    /// V-40 — shared [`nmp_core::substrate::EventIngestDispatcher`] slot.
    pub(crate) ingest_dispatcher_slot:
        Arc<std::sync::RwLock<nmp_core::substrate::EventIngestDispatcher>>,
    /// #1811 — shared crate-registered FTS scope registry.
    pub(crate) search_scope_registry: Arc<nmp_core::substrate::SearchScopeRegistry>,
    /// #1804 — shared crate-registered input-scope recognizer registry.
    pub(crate) input_scope_registry: Arc<nmp_core::substrate::InputScopeRegistry>,
    /// V-40 — shared [`nmp_core::substrate::DmInboxRelayLookup`] slot.
    pub(crate) dm_inbox_relays_slot:
        Arc<Mutex<Arc<dyn nmp_core::substrate::DmInboxRelayLookup>>>,
    /// ADR-0057 PR 2 — shared [`nmp_core::substrate::ProfileLookup`] slot.
    pub(crate) profile_lookup_slot:
        Arc<Mutex<Arc<dyn nmp_core::substrate::ProfileLookup>>>,
    /// ADR-0057 PR 3 — shared [`nmp_core::substrate::ContactsLookup`] slot.
    pub(crate) contacts_lookup_slot:
        Arc<Mutex<Arc<dyn nmp_core::substrate::ContactsLookup>>>,
    /// Substrate [`nmp_core::substrate::BlockedRelayLookup`] slot.
    pub(crate) blocked_relays_slot:
        Arc<Mutex<Arc<dyn nmp_core::substrate::BlockedRelayLookup>>>,
    /// Per-app override for the active-account bootstrap Tailing
    /// self-kinds list.
    pub(crate) bootstrap_self_kinds: Arc<Mutex<Option<Vec<u64>>>>,
    /// H4 — read-only [`nmp_core::substrate::MailboxCache`] handle used by the
    /// `nmp_app_encode_profile` NIP-19 identity encoder.
    pub(crate) mailbox_cache_reader: Mutex<Option<Arc<dyn nmp_core::substrate::MailboxCache>>>,
    /// NIP-50 higher-order search relay source (kind:10007 read seam +
    /// app-default fallback).
    pub(crate) search_relay_source: SearchRelaySourceSlot,
    /// Live NIP-50 search sessions, keyed by host session id.
    pub(crate) search_sessions: Mutex<HashMap<String, crate::search::SearchSession>>,
    /// Test-support GC budget ceiling.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) gc_budget_ceiling: Arc<Mutex<Option<usize>>>,
}
