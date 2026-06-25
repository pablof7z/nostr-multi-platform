//! `NmpApp` struct definition + pre-construction type aliases.
//!
//! Extracted from `lib.rs` to keep each file under the 500-LOC ceiling
//! (AGENTS.md file-size rule). No logic lives here — pure data-structure
//! declarations and slot-constructor helpers.
//!
//! Sub-struct definitions (`CompositionConfig`, `CapabilityPorts`,
//! `ReadHandles`) live in the sibling [`app_sub_structs`] module and are
//! re-exported here so the rest of the crate can import from a single path.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::passive_start::ActorStarter;
use nmp_core::__ffi_internal::{
    ActionRegistry, CapabilityCallbackSlot, KernelEventObserverSlot, LifecycleObserverSlot,
    SnapshotProjectionSlot,
};
use nmp_core::slots::SingletonEventObserverIdSlot;
use nmp_core::KernelEventObserverId;
use std::ffi::c_void;
use std::sync::mpsc;

pub(crate) use crate::app_sub_structs::{CapabilityPorts, CompositionConfig, ReadHandles};

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

pub type IdentityChangeObserverId = u64;
pub(crate) type IdentityChangeCallback = Arc<dyn Fn(Option<String>) + Send + Sync>;

#[derive(Clone)]
pub(crate) struct IdentityChangeObserverRegistration {
    pub(crate) id: IdentityChangeObserverId,
    pub(crate) callback: IdentityChangeCallback,
}

pub(crate) type IdentityChangeObserverSlot = Arc<Mutex<Vec<IdentityChangeObserverRegistration>>>;

pub(crate) fn new_identity_change_observer_slot() -> IdentityChangeObserverSlot {
    Arc::new(Mutex::new(Vec::new()))
}

pub(crate) fn unregister_identity_change_observer(
    observers: &IdentityChangeObserverSlot,
    id: IdentityChangeObserverId,
) {
    if let Ok(mut registrations) = observers.lock() {
        registrations.retain(|registration| registration.id != id);
    }
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

pub(crate) fn read_active_account(slot: &nmp_core::slots::ActiveAccountSlot) -> Option<String> {
    slot.lock().ok().and_then(|guard| guard.clone())
}

pub(crate) fn notify_identity_change_observers(
    active_account: &nmp_core::slots::ActiveAccountSlot,
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

    let callbacks: Vec<IdentityChangeCallback> = observers
        .lock()
        .map(|guard| {
            guard
                .iter()
                .map(|registration| Arc::clone(&registration.callback))
                .collect()
        })
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
    pub(crate) next_identity_change_observer_id: AtomicU64,
    pub(crate) capability_callback: CapabilityCallbackSlot,
    /// T118 / G3 — lifecycle observer slot.
    pub(crate) lifecycle_observer: LifecycleObserverSlot,
    /// T146 — kernel event observer slot.
    pub(crate) event_observers: KernelEventObserverSlot,
    /// Singleton kernel-event observer-id slot used by per-app crates that
    /// register exactly one auxiliary `KernelEventObserver` per app.
    pub(crate) singleton_event_observer_id: SingletonEventObserverIdSlot,
    /// Shared relay-edit rows handle.
    pub(crate) configured_relays: nmp_core::AppRelaySlot,
    /// One-shot account-creation intent.
    pub(crate) pending_mls_autopublish: AtomicBool,
    /// One-shot native actor starter.
    pub(crate) actor_starter: Mutex<Option<ActorStarter>>,
    /// Passive-handle bootstrap frame sender.
    pub(crate) startup_update_tx: Mutex<Option<mpsc::Sender<nmp_core::UpdateFrameBytes>>>,
    pub(crate) actor: Mutex<Option<JoinHandle<()>>>,
    pub(crate) update_listener: Mutex<Option<JoinHandle<()>>>,
    /// M6 — namespace-keyed action-dispatch registry.
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
    /// Test-only monotone send counter.
    #[cfg(test)]
    pub(crate) send_cmd_count: AtomicU64,
    /// Test-only last-command-variant tag.
    #[cfg(test)]
    pub(crate) last_cmd_tag: std::sync::Mutex<Option<&'static str>>,
    /// ADR-0052 §D3 — per-app NIP-46 broker handle.
    #[cfg(feature = "signer-broker")]
    pub(crate) signer_broker: Arc<Mutex<Option<Arc<nmp_signer_broker::BunkerBroker>>>>,
    /// ADR-0052 §D3 — per-app NIP-55 driver handle.
    #[cfg(feature = "external-signer")]
    pub(crate) external_signer_driver: Arc<Mutex<Option<Arc<crate::external_signer::Nip55Driver>>>>,
    /// Live NIP-50 search sessions, keyed by host session id.
    pub(crate) search_sessions: Mutex<HashMap<String, crate::search::SearchSession>>,
    /// Live NIP-29 per-open read views (group chat / discovered / joined),
    /// keyed by the view's (singleton) projection key. Each is a hydrating
    /// observed-interest session torn down on `close_*` (#2088).
    pub(crate) group_feed_sessions: Mutex<HashMap<String, crate::group_feed::GroupFeedSession>>,
    /// Observed-projection sessions keyed by `KernelEventObserverId`. Each
    /// entry maps an observer id returned by `open_observed_projection` to the
    /// close params `(filter_json, consumer_id, scope, relay_pin)` needed to
    /// reverse the open in `close_observed_projection`.
    pub(crate) observed_projection_sessions: Mutex<HashMap<nmp_core::KernelEventObserverId, (String, String, u32, Option<String>)>>,
    /// Test-support GC budget ceiling.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) gc_budget_ceiling: Arc<Mutex<Option<usize>>>,

    // ── Grouped sub-structs ───────────────────────────────────────────────────
    /// Immutable pre-start configuration slots (storage path, NIP-46 config,
    /// substrate factory slots, hooks, interceptors).
    pub(crate) composition: CompositionConfig,
    /// Pluggable substrate lookup/dispatch handles shared with the actor.
    pub(crate) capability_ports: CapabilityPorts,
    /// Handles published back by the actor after kernel construction.
    pub(crate) read_handles: ReadHandles,
}
