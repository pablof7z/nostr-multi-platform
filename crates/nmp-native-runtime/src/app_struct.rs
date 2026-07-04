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
    ActionRegistry, CapabilityCallbackSlot, LifecycleObserverSlot, ObservedProjectionSinkSlot,
    SnapshotProjectionSlot,
};
use nmp_core::ObservedProjectionId;
use std::sync::mpsc;

pub(crate) use crate::app_sub_structs::{CapabilityPorts, CompositionConfig, ReadHandles};

// ── Update-listener types ─────────────────────────────────────────────────

/// Rust-native update listener. The byte slice is a borrowed FlatBuffers
/// update frame valid only for the callback duration.
pub type UpdateListener = Arc<dyn Fn(&[u8]) + Send + Sync + 'static>;

// ── Update-callback quiescence gate ──────────────────────────────────────────

/// Inner mutable state for the update-listener quiescence gate.
///
/// Invariant: `in_flight > 0` only while the listener thread is actively
/// executing a registered listener. The listener increments `in_flight` while
/// holding the mutex (so a concurrent `set_update_callback` cannot observe
/// `in_flight == 0` and return while a listener is still running), then
/// releases the mutex before invoking host code. When the listener
/// returns the listener re-acquires the mutex, decrements `in_flight`, and
/// notifies `UpdateCallbackGate::drained` — which wakes any
/// `set_update_callback` call that is waiting for the old registration to
/// quiesce.
pub(crate) struct UpdateListenerGateInner {
    pub(crate) listener: Option<UpdateListener>,
    pub(crate) in_flight: u32,
}

/// Quiescence-safe slot for native update-listener registration.
///
/// Provides the contract: after `set_update_listener` returns, the previous
/// listener is guaranteed to be neither registered nor mid-invocation. C ABI
/// wrappers build their own listener closure around function pointers; the
/// runtime owns only this Rust-native delivery/drain mechanic.
pub(crate) struct UpdateListenerGate {
    pub(crate) inner: Mutex<UpdateListenerGateInner>,
    pub(crate) drained: std::sync::Condvar,
}

impl UpdateListenerGate {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(UpdateListenerGateInner {
                listener: None,
                in_flight: 0,
            }),
            drained: std::sync::Condvar::new(),
        }
    }
}

pub(crate) type UpdateListenerSlot = Arc<UpdateListenerGate>;

pub(crate) fn new_update_listener_slot() -> UpdateListenerSlot {
    Arc::new(UpdateListenerGate::new())
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

pub(crate) type ConfiguredRelaysChangeCallback = Arc<dyn Fn() + Send + Sync>;
pub(crate) type ConfiguredRelaysChangeObserverSlot =
    Arc<Mutex<Vec<ConfiguredRelaysChangeCallback>>>;

pub(crate) fn new_configured_relays_change_observer_slot() -> ConfiguredRelaysChangeObserverSlot {
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

pub(crate) fn notify_configured_relays_change_observers(
    configured_relays: &nmp_core::AppRelaySlot,
    last_notified: &Arc<Mutex<Vec<(String, String)>>>,
    observers: &ConfiguredRelaysChangeObserverSlot,
) {
    let current = configured_relays
        .lock()
        .map(|rows| {
            rows.as_slice()
                .iter()
                .map(|row| (row.url().to_string(), row.role().to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    {
        let Ok(mut last) = last_notified.lock() else {
            return;
        };
        if *last == current {
            return;
        }
        *last = current;
    }

    let callbacks = observers
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    for callback in callbacks {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback()));
    }
}

// ── NmpApp struct ─────────────────────────────────────────────────────────────

pub struct NmpApp {
    pub(crate) tx: nmp_core::CommandSender,
    pub(crate) update_listener: UpdateListenerSlot,
    /// Rust-side active-account observer registry. The update listener fires
    /// callbacks after the actor has written `active_account_handle` and before
    /// it forwards the same update frame to the native callback.
    pub(crate) identity_change_observers: IdentityChangeObserverSlot,
    pub(crate) next_identity_change_observer_id: AtomicU64,
    pub(crate) configured_relays_change_observers: ConfiguredRelaysChangeObserverSlot,
    pub(crate) capability_callback: CapabilityCallbackSlot,
    /// T118 / G3 — lifecycle observer slot.
    pub(crate) lifecycle_observer: LifecycleObserverSlot,
    /// Declared observed-projection sink slot.
    pub(crate) event_observers: ObservedProjectionSinkSlot,
    /// Shared relay-edit rows handle.
    pub(crate) configured_relays: nmp_core::AppRelaySlot,
    /// One-shot native actor starter.
    pub(crate) actor_starter: Mutex<Option<ActorStarter>>,
    /// Passive-handle bootstrap frame sender.
    pub(crate) startup_update_tx: Mutex<Option<mpsc::Sender<nmp_core::UpdateFrameBytes>>>,
    pub(crate) actor: Mutex<Option<JoinHandle<()>>>,
    pub(crate) update_listener_thread: Mutex<Option<JoinHandle<()>>>,
    /// M6 — namespace-keyed action-dispatch registry.
    pub(crate) action_registry: ActionRegistry,
    /// ADR-0069 Part 2 — the composition ledger.
    pub(crate) composition_ledger: Arc<nmp_core::CompositionLedger>,
    /// ADR-0069 Part 2 — flips to `true` the first time `nmp_app_start` sends
    /// `ActorCommand::Start`.
    pub(crate) started: AtomicBool,
    /// Host-extensible typed snapshot output registry.
    pub(crate) snapshot_projections: SnapshotProjectionSlot,
    /// Reusable feed-controller registry.
    pub(crate) feed_registry: nmp_feed::FeedRegistrySlot,
    /// #1740 step 2 — the feed-SESSION registry.
    pub(crate) feed_sessions: Arc<nmp_feed::FeedSessionRegistry>,
    /// #1740 step 4 — app-registered custom feed policy definitions.
    pub(crate) custom_feed_policies: Arc<nmp_feed::CustomFeedPolicyRegistry>,
    /// G-S4 — straddle counter for the actor command channel depth.
    pub(crate) queue_depth: Arc<AtomicU64>,
    /// Test-only monotone send counter.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) send_cmd_count: AtomicU64,
    /// Test-only last-command-variant tag.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) last_cmd_tag: std::sync::Mutex<Option<&'static str>>,
    /// ADR-0072 §D3 — per-app NIP-46 actor-lane runtime handle (set during
    /// config phase by `nmp_signer_broker_init`; `None` until then).
    #[cfg(feature = "signer-broker")]
    pub(crate) nip46_runtime: Arc<Mutex<Option<nmp_nip46_runtime::Nip46RuntimeHandle>>>,
    /// ADR-0072 §D3 — per-app NIP-55 driver handle.
    #[cfg(feature = "external-signer")]
    pub(crate) external_signer_driver: Arc<Mutex<Option<Arc<crate::external_signer::Nip55Driver>>>>,
    /// #2927 — app-injected NIP-AD auto-resolution policy. `None` (the default)
    /// means the "explicit only" posture (`NeverAutoResolve`): the content
    /// renderer never passively fetches an AD URL. The app installs a policy at
    /// its composition root (identical to registering a content component for a
    /// kind); only moment-1 (passive render) consults it — moment-2 (explicit
    /// paste/search) is never policy-gated.
    #[cfg(feature = "nip-ad")]
    pub(crate) ad_resolution_policy:
        Arc<Mutex<Option<Arc<dyn nmp_nip_ad::AdResolutionPolicy>>>>,
    /// #2927 — per-URL NIP-AD resolution state (render-side read-door + claim
    /// refcount). Shared with off-thread resolve workers.
    #[cfg(feature = "nip-ad")]
    pub(crate) ad_url_states: crate::ad::AdUrlStateMap,
    /// Observed-projection sessions keyed by `ObservedProjectionId`. Each
    /// entry maps an observer id returned by `open_observed_projection` to the
    /// close params `(filter_json, consumer_id, scope, relay_pin,
    /// is_indexer_discovery)` needed to reverse the open in
    /// `close_observed_projection`.
    pub(crate) observed_projection_sessions:
        Arc<Mutex<HashMap<ObservedProjectionId, (String, String, u32, Option<String>, bool)>>>,
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
