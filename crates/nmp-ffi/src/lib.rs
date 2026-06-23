//! Path-A raw C FFI surface. `mod.rs` carries the lifecycle wrappers + shared
//! argument helpers; `identity` carries the T66a identity / multi-account /
//! relay-edit wrappers; `publish` carries the publish-handle entry points
//! (signed/unsigned event publish, retry, cancel) — split out of `identity`
//! per AGENTS.md "co-locate by owner, not by role"; `timeline` carries the
//! open/close + profile claim/release wrappers; `testing` carries the
//! cfg-gated injectors (split to keep each file under the 300-LOC soft cap).

// Quiescence contract for `nmp_app_set_update_callback`: after the setter
// returns, the old `(callback, context)` pair is guaranteed not to be
// mid-invocation. Tests exercise `UpdateCallbackGate` directly (no full app
// stack) via the `pub(crate)` gate fields.
#[cfg(test)]
#[path = "passive_start_tests.rs"]
mod passive_start_tests;
#[cfg(test)]
#[path = "update_callback_quiescence_tests.rs"]
mod update_callback_quiescence_tests;
// Bug 1 (D6 fail-loud) — `NmpApp::remove_account_forgetting_keyring` checks the
// keyring forget result and does NOT remove the account when the keychain
// reports `Error` (otherwise the nsec is orphaned in the keychain). Lives in
// its own module to keep `lib.rs` under its LOC ceiling.
mod keyring_forget;
// V-82 — `NmpApp::active_account_handle()` single-source-of-truth tests
// (real sign-in / switch / Reset driven through the actor thread).
#[cfg(test)]
#[path = "active_account_handle_tests.rs"]
mod active_account_handle_tests;
// V-83 — `NmpApp::event_by_id()` synchronous event-read tests (real event
// ingest driven through the actor thread; publish-back slot survives Reset).
mod action;
mod app_config_hooks;
mod app_config_search; // #1811: `register_search_scope` (impl NmpApp; LOC ceiling).
mod app_config_intent; // #1804: `register_input_scope` (impl NmpApp; LOC ceiling).
mod app_config_substrate;
mod app_host_impl; // ADR-0053: `impl AppHost for NmpApp` extracted here (LOC ceiling).
mod capability;
mod declared_projections; // ADR-0053/E4: `impl NmpApp` consumed-projection-intent methods (LOC ceiling).
mod following_count; // `nmp_app_active_following_count` — live kind:3 follow-count read for profile headers.

// Canonical cross-cutting string-free symbol. Every `*mut c_char` returned
// by any NMP FFI function must be freed via `nmp_free_string`.
#[cfg(test)]
#[path = "event_by_id_tests.rs"]
mod event_by_id_tests;
mod free;
mod passive_start;
mod prestart_config;
// D13 sign-and-return — `nmp_app_sign_event_for_return` end-to-end through the
// actor thread, reading the `signed_events` projection.
#[cfg(test)]
#[path = "sign_event_for_return_tests.rs"]
mod sign_event_for_return_tests;
// M2 (ADR-0042 §5.1, V-112) — register_feed_with_observer / unregister_feed
// transient-feed teardown-seam tests.
mod event_observer;
mod feed;
// #1740 step 2 — `NmpApp::open_feed` / `close_feed` session registry over the
// existing feed mechanics. Rust-level seam only (the C/wasm surface is step 7).
mod feed_session;
mod identity;
#[cfg(test)]
#[path = "interest_feed_tests.rs"]
mod interest_feed_tests;
mod lifecycle;
// H4 — NMP-provided NIP-19 identity encoder (`nmp_app_encode_profile`). Split
// out of `identity.rs` under the same owner namespace to keep both files
// under the LOC ceiling; the `#[no_mangle]` symbol name is ABI-stable across
// the split (same precedent as `publish.rs` ← `identity.rs`).
mod nip19_ffi;
// #1804 — input-intent resolver C-ABI (`nmp_app_intent_classify` /
// `nmp_app_intent_dispatch`): classify a one-box / paste / search input via the
// pure `nmp_intent::classify`, then (dispatch) route the top candidate to its
// existing seam (open-uri / search / NIP-05 reverse lookup).
mod intent_ffi;
// Issue #1554 — stateless NIP-21 / bare NIP-19 decode-to-wire helper.
// Decode-only: no actor command, no view mutation, no app-specific policy.
mod nip21_ffi;
mod publish;
// ADR-0058 §3 (step 3b) — synchronous read-only pull-page C-ABI surface.
pub mod pull;
mod relay_config;
#[cfg(feature = "signer-broker")]
mod signer_broker;
// ADR-0052 §D3 — per-app signer-port accessors (`impl NmpApp` block split out
// of `lib.rs`); methods individually feature-gated, module always compiled.
mod signer_ports;
// ADR-0055 Rung 3 / R6-S1 — incremental-apply + frame-identity accessors
// (`impl NmpApp` block split out of `lib.rs` for file-size discipline).
mod incremental_apply;
// ADR-0048 Stage 2 — NIP-55 external-signer driver (capability-bridge
// transport + first-connect flow + actor re-entry).
#[cfg(feature = "external-signer")]
mod external_signer;
// V-51 phase 2 — routing-trace FFI snapshot accessor
// (`nmp_app_recent_routing_decisions`). Pull-only diagnostic surface; not
// folded into the snapshot tick.
mod routing_trace;
// ADR-0049 Part 2 — composition-report pull accessor
// (`nmp_app_composition_report`). Pull-only diagnostic surface; not folded into
// the snapshot tick.
mod composition_report;
// ADR-0063 Lane D — unified `nmp_app_resolve_ref` / `nmp_app_release_ref` C-ABI
// symbols. Generalizes the former per-kind profile claim + claim_event behind one
// origin-blind seam. Lane H deleted the per-kind profile claim/release symbols;
// profiles resolve exclusively through resolve_ref (claim_event is retained).
mod resolve_ref;
// Higher-order NIP-50 search: `nmp_app_search_open` / `_close` / `_snapshot`
// C-ABI + the `NmpApp::open_search` Rust API hl calls directly. Reusable by
// every NmpApp host; orchestration primitives live in `nmp-nip50`.
mod search;
mod snapshot;
mod storage;
mod timeline;
// #1607: `mod wallet` deleted. The bespoke `nmp_app_wallet_*` FFI shims
// (`nmp_app_wallet_connect`, `nmp_app_wallet_disconnect`,
// `nmp_app_wallet_pay_invoice`) are gone. Callers use
// `nmp_app_dispatch_action("nmp.wallet.connect"|"nmp.wallet.disconnect"|
// "nmp.wallet.pay_invoice", action_json)` directly. The bolt11 double-tap
// guard moved into `nmp_nip47::action::WalletPayInvoiceModule` (owned by
// value, ADR-0052 rung 5.2) and the `inflight_bolt11` field was removed
// from `NmpApp`.

#[cfg(any(test, feature = "test-support"))]
mod testing;
#[cfg(any(test, feature = "test-support"))]
mod testing_sync;

// ADR-0052 §D3 — test-support seam for the rung 5.3 per-app signer-port
// oracle. Gated test-only; never in the production FFI ABI.
#[cfg(any(test, feature = "test-support"))]
mod signer_ports_test_support;

// ── Native re-export surface ──────────────────────────────────────────────
// Hoist every per-submodule FFI entry-point into the `ffi::` namespace so
// any native (non-WASM) Rust-side caller — composition-root crates
// (`nmp-defaults`, `nmp-app-*`), out-of-crate integration tests, the
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
// test-support delta only adds the harness-only injectors / read helpers.
//
// `allow(unused_imports)`: in-crate `tests` modules reach these symbols by
// their `super::` / module path, so the facade re-export is only consumed by
// out-of-crate clients; keeps `cargo test -p nmp-core --lib` clean.
// ADR-0064 / Cut-B (#1756): `nmp_app_dispatch_action` (JSON doorway) deleted;
// `nmp_app_dispatch_action_bytes` is the sole remaining dispatch entry point.
#[cfg(feature = "native")]
#[allow(unused_imports)]
pub use action::{
    nmp_app_ack_action_stage, nmp_app_dispatch_action_bytes,
    nmp_app_register_action_result_observer,
};
// Test-support shim: re-export the deleted JSON doorway for integration tests
// in sibling crates that have not yet been migrated to the typed byte path.
// Never compiled into production binaries (only under test-support feature).
#[cfg(feature = "test-support")]
pub use action::nmp_app_dispatch_action;
#[cfg(feature = "native")]
pub use capability::{nmp_app_dispatch_capability, nmp_app_set_capability_callback};
#[cfg(feature = "native")]
pub use event_observer::{nmp_app_register_event_observer, nmp_app_unregister_event_observer};
#[cfg(feature = "native")]
pub use feed::nmp_app_load_older_feed;
// #1740 step 1 — typed feed-session param types + boundary decode/validation
// (no `open_feed` dispatch yet; that is step 2).
#[cfg(feature = "native")]
pub use feed::{
    decode_and_validate_feed_params, validate_feed_params, FeedAdmission, FeedHandle, FeedParams,
    FeedParamsDecodeError, FeedParamsError, FeedRanking, FeedScope, FeedSessionId, FeedWindow,
    ProjectionKey, PubkeySetExpr,
};
// #1740 step 2 — `open_feed`/`close_feed` session-registry seam (Rust-level).
#[cfg(feature = "native")]
pub use feed_session::{
    handle_projection_key, FeedCompileOutput, FeedCompiler, FeedOpenError, FeedTeardown,
};
#[cfg(feature = "native")]
pub use free::nmp_free_string;
#[cfg(feature = "native")]
pub use identity::{
    create_new_account_with_initial_follows, nmp_app_add_relay, nmp_app_create_new_account,
    nmp_app_register_agent_nsec, nmp_app_remove_account, nmp_app_remove_relay,
    nmp_app_signin_bunker, nmp_app_signin_nsec, nmp_app_switch_active,
};
// V-68 Stage 2 (ADR-0042 amendment 2026-06-12): nmp_app_open_timeline DELETED.
// Use nmp_app_chirp_open_home_feed (Chirp-specific wrapper). The old generic
// old feed-open symbols remain compatibility shims only.
#[cfg(feature = "native")]
#[allow(unused_imports)]
pub use lifecycle::{
    nmp_app_is_alive, nmp_app_lifecycle_background, nmp_app_lifecycle_foreground,
    nmp_app_set_lifecycle_callback,
};
#[cfg(feature = "native")]
pub use nip19_ffi::nmp_app_encode_profile;
#[cfg(feature = "native")]
pub use nip21_ffi::nmp_nip21_decode_uri;
// Publish-lifecycle control-plane FFI (retry/cancel). The one-door-per-
// capability rule deleted the bespoke event-producing siblings
// (`nmp_app_publish_signed_event` / `nmp_app_publish_signed_event_to` /
// `nmp_app_publish_unsigned_event`) — every event-producing publish now
// goes through `nmp_app_dispatch_action` (`nmp.publish`). Retry addresses a
// publish *handle*; cancel (S7, #1754) addresses the operation
// `correlation_id` (`nmp_app_cancel_action`, replacing the bespoke
// `nmp_app_cancel_publish`). Neither has a `dispatch_action` equivalent, so
// they stay on these dedicated control-plane symbols (the D11 lint whitelists
// them).
#[cfg(feature = "native")]
pub use publish::{nmp_app_cancel_action, nmp_app_retry_publish};
// V-51 phase 2 — routing-trace JSON accessor. Pull-only; the returned
// pointer is heap-owned and must be freed via `nmp_free_string`.
pub use composition_report::nmp_app_composition_report;
#[cfg(feature = "external-signer")]
pub use external_signer::{
    nmp_app_deliver_external_signer_response, nmp_app_signin_nip55, nmp_external_signer_init,
};
pub use prestart_config::NmpConfigStatus;
#[cfg(feature = "native")]
#[allow(unused_imports)]
pub use routing_trace::nmp_app_recent_routing_decisions;
#[cfg(feature = "signer-broker")]
pub use signer_broker::{
    nmp_app_cancel_bunker_handshake, nmp_app_nostrconnect_uri, nmp_signer_broker_init,
};
#[cfg(feature = "native")]
#[allow(unused_imports)]
pub use snapshot::{
    nmp_app_consume_all_builtin_projections, nmp_app_declare_consumed_projections,
    nmp_app_declare_incremental_apply,
};
#[cfg(feature = "native")]
pub use storage::nmp_app_set_storage_path;
pub use following_count::nmp_app_active_following_count;
#[cfg(feature = "native")]
pub use timeline::{
    // V-68 / V-112 (ADR-0042): nmp_app_open_author, nmp_app_close_author,
    // nmp_app_open_thread, nmp_app_close_thread deleted from timeline.rs.
    // V-68 Stage 2 (ADR-0042 amendment 2026-06-12): nmp_app_open_timeline
    // deleted from identity.rs.
    // ADR-0063 Lane H: nmp_app_claim_profile, nmp_app_release_profile deleted.
    // #1740 step 8: `nmp_app_open_contact_feed` / `nmp_app_close_contact_feed`
    // C-ABI shims DELETED. `declare_active_follows_feed` / `clear_active_follows_feed`
    // stay as INTERNAL composition glue (home-feed wiring), not app-facing C ABI.
    clear_active_follows_feed,
    declare_active_follows_feed,
    nmp_app_claim_event,
    nmp_app_close_interest,
    nmp_app_open_interest,
    nmp_app_open_uri,
    nmp_app_release_event,
};
// ADR-0063 Lane D — unified ref-resolution C-ABI entry points. Lane H deleted the
// per-kind profile claim/release symbols; these are the sole profile-resolution
// surface (the event claim/release URI front-door is retained alongside them).
#[cfg(feature = "native")]
pub use resolve_ref::{nmp_app_release_ref, nmp_app_resolve_ref};

// ── test-support delta ───────────────────────────────────────────────────
// Live-bench harnesses (`live-bench`) and integration test binaries
// (`nmp-testing`) need a few extra entry points that production app crates
// do not — pre-verified event injection (S3/S4/S5 throughput harnesses)
// and read-side projection JSON dumps (assert reducer output without going
// through the snapshot callback). Per-action stage ACK is part of the public
// native C ABI and is re-exported above for Android JNI parity.
#[cfg(any(test, feature = "test-support"))]
pub use testing::{
    nmp_app_configure_gc_budget, nmp_app_inject_pre_verified_events,
    nmp_app_inject_signed_event_json, nmp_app_inject_signed_events,
    nmp_app_inject_unpinned_events_for_gc, nmp_app_read_author_event_ids,
    nmp_app_read_projection_churn_stats, nmp_app_read_ram_eviction_stats, nmp_app_trigger_gc_step,
};
#[cfg(any(test, feature = "test-support"))]
pub use testing_sync::nmp_app_wait_barrier;
// ADR-0052 §D3 — rung 5.3 per-app signer-port oracle seam.
#[cfg(any(test, feature = "test-support"))]
pub use signer_ports_test_support::{
    install_bunker_hook_for_test, install_external_signer_hook_for_test,
    invoke_bunker_connect_hook_for_test, invoke_external_signer_restore_hook_for_test,
};

// ── android-ffi delta ────────────────────────────────────────────────────
// `nmp_app_remove_account`, `nmp_app_signin_bunker`, and `nmp_app_switch_active`
// were historically gated here; they are lifecycle essentials every native app
// needs and are now included unconditionally in the `native` block above.
// The android-ffi identity block is intentionally removed.
// #1607: `nmp_app_wallet_{connect,disconnect,pay_invoice}` deleted — callers
// use `nmp_app_dispatch_action` directly (D11 — one publish door).

// Step 11 final — the FFI shell was extracted from `nmp-core::ffi` into
// this crate (`nmp-ffi`). nmp-core re-exports the items the shell reaches
// for through `nmp_core::__ffi_internal::*` (substrate-grade slot
// constructors, registration helpers, default constants); everything
// already on the public surface comes through `nmp_core::*` directly.
use nmp_core::__ffi_internal::{
    default_registry, dispatch_capability, new_app_relay_slot, new_bunker_handshake_slot,
    new_capability_callback_slot, new_event_observer_slot, new_lifecycle_observer_slot,
    new_signer_state_slot, new_snapshot_projection_slot, register_rust_observer,
    register_rust_observer_muted, run_actor_with_observers, unregister_observer, ActionRegistry,
    ActorChannels, ActorConfigSources, ActorRuntimeSlots, CapabilityCallbackSlot,
    KernelEventObserverSlot, LifecycleObserverSlot, SnapshotProjectionSlot, DEFAULT_EMIT_HZ,
    DEFAULT_VISIBLE_LIMIT,
};
// V-38: the `new_wallet_status_slot` re-export moved to `nmp-nip47`; the
// host (per-app crate) constructs the slot and registers the typed
// `"wallet"` sidecar via `register_typed_snapshot_projection` (ADR-0037).
use nmp_core::slots::{
    event_by_id_from_store, new_active_account_slot, new_active_local_keys_slot,
    new_event_store_slot, new_external_event_sink_policy_slot, new_mls_local_nsec_slot,
    new_nostrconnect_bootstrap_relay_slot, new_nostrconnect_perms_slot, new_publish_resolver_slot,
    new_pull_cursor_registry_handle_slot, new_routing_substrate_slot, new_routing_trace_slot,
    new_singleton_event_observer_id_slot, new_storage_path_slot, ActiveAccountSlot,
    ActiveLocalKeysSlot, EventStoreSlot, ExternalEventSinkPolicySlot, MlsLocalNsecSlot,
    NostrConnectBootstrapRelaySlot, NostrConnectPermsSlot, PublishResolverSlot,
    PullCursorRegistryHandleSlot, RoutingSubstrateSlot, RoutingTraceSlot,
    SingletonEventObserverIdSlot, StoragePathSlot,
};
use nmp_core::subs::PlanCoverageHook;
use nmp_core::substrate::new_external_event_sink_dispatcher_slot;
use nmp_core::{ActorCommand, KernelEventObserver, KernelEventObserverId};
use passive_start::{prestart_snapshot_frame, ActorStarter};
use std::ffi::{c_char, c_uint, c_void, CStr};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::thread::JoinHandle;
use zeroize::Zeroizing;

pub(crate) type UpdateCallback = extern "C" fn(*mut c_void, *const u8, usize);

#[derive(Clone, Copy)]
pub(crate) struct UpdateCallbackRegistration {
    pub(crate) context: usize,
    pub(crate) callback: UpdateCallback,
}

// Step 11 final — the slot type aliases + constructors that used to live
// here (MlsLocalNsecSlot, ActiveLocalKeysSlot, StoragePathSlot,
// RoutingTraceSlot, RoutingSubstrateSlot, SingletonEventObserverIdSlot,
// and their `new_*_slot` constructors) moved to `nmp-core::slots` so the
// actor (a crate-private module inside
// `nmp-core`) can still name them after the FFI extraction. The aliases
// are imported through the `use nmp_core::slots::*` block at the top of
// this file.

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
    pub(crate) drained: Condvar,
}

impl UpdateCallbackGate {
    fn new() -> Self {
        Self {
            inner: Mutex::new(UpdateCallbackGateInner {
                registration: None,
                in_flight: 0,
            }),
            drained: Condvar::new(),
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
type UpdateCallbackSlot = Arc<UpdateCallbackGate>;

fn new_update_callback_slot() -> UpdateCallbackSlot {
    Arc::new(UpdateCallbackGate::new())
}

type IdentityChangeCallback = Arc<dyn Fn(Option<String>) + Send + Sync>;
type IdentityChangeObserverSlot = Arc<Mutex<Vec<IdentityChangeCallback>>>;

fn new_identity_change_observer_slot() -> IdentityChangeObserverSlot {
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
type SearchRelaySourceSlot =
    Arc<Mutex<Option<Arc<dyn nmp_core::substrate::PreferredRelaySource>>>>;

fn new_search_relay_source_slot() -> SearchRelaySourceSlot {
    Arc::new(Mutex::new(None))
}

fn read_active_account(slot: &ActiveAccountSlot) -> Option<String> {
    slot.lock().ok().and_then(|guard| guard.clone())
}

fn notify_identity_change_observers(
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

pub struct NmpApp {
    tx: nmp_core::CommandSender,
    update_callback: UpdateCallbackSlot,
    /// Rust-side active-account observer registry. The update listener fires
    /// callbacks after the actor has written `active_account_handle` and before
    /// it forwards the same update frame to the native callback.
    identity_change_observers: IdentityChangeObserverSlot,
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
    /// The slot is a typed [`nmp_core::AppRelaySlot`]
    /// (`Arc<Mutex<AppRelayList>>`) — D14 forbids new bare
    /// `Arc<Mutex<Vec<…>>>` fields on `NmpApp` and the typed wrapper makes
    /// the slot's purpose visible at the declaration site.
    configured_relays: nmp_core::AppRelaySlot,
    /// Pre-start initial relay configuration. Set by `NmpAppBuilder::start()`
    /// (Rust path) or by calling `set_initial_relays_for_start` before
    /// `nmp_app_start`. The `nmp_app_start` function reads this slot and carries
    /// it in `ActorCommand::Start { initial_relays }`.
    ///
    /// D14: `Arc<Mutex<Vec<(String, String)>>>` is NOT the banned
    /// `Arc<Mutex<Vec<RowType>>>` projection shape — it is a one-shot pre-start
    /// staging slot consumed once by `nmp_app_start`, never shared with the
    /// actor thread as a live projection.
    initial_relays_for_start: Mutex<Vec<(String, String)>>,
    /// V-65 — host-supplied bootstrap relay URL for client-initiated NIP-46
    /// `nostrconnect://` handshakes when the user has no configured write relay.
    ///
    /// Written by the composition root (via [`AppHost::set_nostrconnect_bootstrap_relay`])
    /// before `nmp_app_start`; read on the FFI thread when
    /// [`NmpApp::nostrconnect_relay_url`] is called. `None` (the default)
    /// means no bootstrap relay was registered: the caller receives an error
    /// rather than a silent third-party-URL fallback (D0 / V-65).
    ///
    /// D14: `Arc<Mutex<Option<String>>>` is NOT the banned
    /// `Arc<Mutex<Vec<…>>>` shape. The slot is not shared with the actor
    /// thread — no actor clone is handed to `run_actor_with_observers`.
    nostrconnect_bootstrap_relay: NostrConnectBootstrapRelaySlot,
    /// #1493 P9 — host-supplied NIP-46 permission request for client-initiated
    /// `nostrconnect://` handshakes (which event kinds the app asks the signer
    /// to sign).
    ///
    /// Written by the composition root (via [`AppHost::set_nostrconnect_perms`])
    /// before `nmp_app_start`; read on the FFI thread when
    /// [`NmpApp::nostrconnect_perms`] is called. `None` (the default) means NMP
    /// supplies NO perms — the handshake omits the `&perms=` parameter entirely.
    /// The perm set is leaf-app product policy, not framework policy (#1493).
    ///
    /// D14: `Arc<Mutex<Option<String>>>` is NOT the banned `Arc<Mutex<Vec<…>>>`
    /// shape — same shape as `nostrconnect_bootstrap_relay`. The slot is not
    /// shared with the actor thread.
    nostrconnect_perms: NostrConnectPermsSlot,
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
    /// V-82 — the active account's raw hex pubkey, or `None` when no account
    /// is signed in. This is the SAME `Arc` the kernel actor writes on every
    /// identity mutation (`Kernel::set_accounts` → `active_account_handle`):
    /// `nmp_app_new` constructs it once, hands an `Arc::clone` to the actor
    /// (which threads it into the kernel at construction and re-hands it on
    /// `Reset`), and keeps this clone for the read side. So a value read
    /// through [`NmpApp::active_account_handle`] reflects real account state —
    /// no divergent mirror.
    ///
    /// Substrate-generic: the actor names no NIP when writing this slot (raw
    /// pubkey `String`, D0). The accessor backs the OP-feed composition root,
    /// while [`NmpApp::register_identity_change_observer`] provides the push
    /// seam for per-account reset work after this slot changes.
    active_account_handle: ActiveAccountSlot,
    /// V-83 — the kernel's `EventStore` handle, published back by the actor
    /// right after kernel construction (and re-published on `Reset`). Unlike
    /// `active_account_handle` (host-constructed, handed down), the store is
    /// **kernel-built** (`build_event_store` inside the kernel constructor), so
    /// this follows the `routing_trace` publish-back pattern: `nmp_app_new`
    /// mints an empty slot, hands an `Arc::clone` to the actor, and the actor
    /// (the sole writer per D4) publishes `kernel.event_store_handle()` into it.
    ///
    /// Host code reads through it synchronously via [`NmpApp::event_by_id`] —
    /// the V-80 OP-feed engine's repost L-2/L-5 backward-hydration paths resolve
    /// a locally-cached parent/target event id this way instead of degrading to
    /// "no card". Substrate-generic: an event id maps to a `KernelEvent` with no
    /// NIP noun (D0). `None` before `nmp_app_start` (the cold-start state).
    event_store_handle: EventStoreSlot,
    /// ADR-0058 step 3b — the kernel's pull-cursor registry handle, published
    /// back by the actor right after kernel construction (and re-published on
    /// `Reset`), same publish-back contract as `event_store_handle`. The
    /// synchronous [`nmp_app_pull_page`](crate::pull::nmp_app_pull_page) read
    /// path snapshots a `PullCursorRegistration` through this slot before
    /// touching the store. `None` before `nmp_app_start`.
    pull_cursor_registry: PullCursorRegistryHandleSlot,
    /// FFI-supplied persistent storage directory for the LMDB `EventStore`
    /// backend. Set by [`nmp_app_set_storage_path`] before
    /// [`nmp_app_start`]. The C-ABI setter writes through this slot; actor
    /// startup snapshots the value and uses it whenever it constructs a
    /// kernel (`run_actor_with_observers` → `Kernel::with_storage_path` →
    /// `build_event_store`), including `Reset`.
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
    /// writes a closure here via [`Self::set_routing_substrate`]; actor
    /// startup snapshots it, then invokes [`crate::kernel::Kernel::set_routing`]
    /// with the produced
    /// `(Arc<dyn OutboxRouter>, Arc<dyn MailboxCache>)` pair. `None` (the
    /// default) leaves the kernel's `EmptyOutboxRouter` + (test-only)
    /// `TestInMemoryMailboxCache` defaults in place (substrate-honest
    /// debt B, 2026-05-24).
    routing_substrate: RoutingSubstrateSlot,
    /// Spec §271 (2026-05-25) — per-app substrate-publish-resolver
    /// factory slot. The per-app crate (today:
    /// `nmp_defaults::register_defaults`) writes a closure here via
    /// [`Self::set_publish_resolver_factory`]; actor startup snapshots it, then
    /// invokes [`crate::kernel::Kernel::set_publish_resolver`] with the produced
    /// `Arc<dyn nmp_core::publish::OutboxResolver>`. `None` (the default)
    /// leaves the kernel's `NoopOutboxResolver` default in place — every
    /// publish through `PublishTarget::Auto` then surfaces `NoTargets`
    /// (fail-closed). Mirrors `routing_substrate` exactly so the actor
    /// pair-applies both factories in one block.
    publish_resolver: PublishResolverSlot,
    /// Test-support kernel-clock injection slot. Default `None` — the kernel
    /// keeps its `SystemClock`. Only [`Self::set_kernel_clock_for_test`]
    /// (compiled under `test` / `test-support`) ever writes it; the actor
    /// reads it once after kernel construction and applies it via
    /// `Kernel::set_clock`. Lets deterministic e2e tests stamp
    /// strictly-increasing `created_at` on replaceable publishes without a
    /// wall-clock sleep (D8).
    kernel_clock: nmp_core::slots::KernelClockSlot,
    /// External event sink policy factory slot.  The dispatcher uses this
    /// to route `SignedEventFrame`s via the new external-sink path.
    external_event_sink_policy: ExternalEventSinkPolicySlot,
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
    /// One-shot native actor starter. `nmp_app_start` consumes it, so
    /// pre-start commands queue without constructing the kernel.
    actor_starter: Mutex<Option<ActorStarter>>,
    /// Passive-handle bootstrap frame sender, dropped at start/drop.
    startup_update_tx: Mutex<Option<mpsc::Sender<nmp_core::UpdateFrameBytes>>>,
    actor: Mutex<Option<JoinHandle<()>>>,
    update_listener: Mutex<Option<JoinHandle<()>>>,
    /// M6 — namespace-keyed action-dispatch registry backing
    /// [`action::nmp_app_dispatch_action_bytes`]. Holds only stateless ZST module
    /// adapters, so it is `Send + Sync` and is queried directly on the FFI
    /// thread (no actor round-trip): registered modules' `start` methods are
    /// pure validators. The `Kernel`-side wiring (execution + the durable
    /// action ledger) is the M6 follow-up; see
    /// [`crate::kernel::action_registry`].
    action_registry: ActionRegistry,
    /// ADR-0049 Part 2 — the composition ledger. A shared
    /// `Arc<CompositionLedger>` recorded at every host-init registration seam
    /// (action registry, ingest parsers, snapshot projections, the
    /// last-writer-wins wiring slots) and the post-start late-wiring drop path.
    /// Read back as JSON by `nmp_app_composition_report`. The SAME handle is
    /// installed on `action_registry` (via `with_composition_ledger`) so action
    /// dispositions land here too. Written only at registration time — never on
    /// a hot path (D8).
    composition_ledger: Arc<nmp_core::CompositionLedger>,
    /// ADR-0049 Part 2 — flips to `true` the first time `nmp_app_start` sends
    /// `ActorCommand::Start`. After that point the actor has read every wiring
    /// slot once at kernel construction, so any setter call is dropped and
    /// recorded as `DroppedLateWiring`. A plain `AtomicBool`
    /// (single-flag, lock-free; same posture as `pending_mls_autopublish`).
    started: AtomicBool,
    /// Host-extensible typed snapshot output registry — the output-side counterpart
    /// to `action_registry`. Shared `Arc<Mutex<…>>` with the actor thread
    /// (bound onto the kernel via `set_snapshot_projection_handle`): a host
    /// registers a typed FlatBuffers projection closure through
    /// [`Self::register_typed_snapshot_projection`], and the kernel runs every
    /// registered closure in `make_update`. Unlike `action_registry`, this is NOT
    /// queried on the FFI thread — it fires from inside the actor tick, hence
    /// the shared-`Arc` slot rather than a plain owned field.
    snapshot_projections: SnapshotProjectionSlot,
    /// Reusable feed-controller registry. App crates register named feed
    /// surfaces here; shells report viewport intent through generic feed FFI
    /// instead of app-specific cursor/window APIs.
    feed_registry: nmp_feed::FeedRegistrySlot,
    /// #1740 step 2 — the feed-SESSION registry: one record per live
    /// [`Self::open_feed`] call owning that feed's full teardown recipe
    /// (projection key, observer/interest ids, pull controller, typed sidecar).
    /// [`Self::close_feed`] looks the session up by the returned handle's id and
    /// runs its teardown idempotently. Engine-agnostic (it stores opaque
    /// teardown closures the compiler supplied — D0/D4); not a second feed
    /// engine, a session WRAPPER over the existing `register_feed*` mechanics.
    feed_sessions: Arc<nmp_feed::FeedSessionRegistry>,
    /// #1740 step 4 — app-registered custom-perspective definitions (closed
    /// data, keyed by opaque id). See [`Self::register_custom_perspective`].
    custom_perspectives: Arc<nmp_feed::PerspectiveRegistry>,
    /// Per-open feed → ingest-observer bookkeeping for *transient* feeds (a
    /// visited profile / open thread registered through
    /// [`Self::register_feed_with_observer`]). The home feed is NOT here — it
    /// registers its observer once via [`Self::register_event_observer`] and
    /// keeps it for the life of the app. A transient feed must instead drop
    /// its `KernelEventObserver` when its screen closes, or the kernel keeps
    /// fanning every ingested event into a dead feed (an unbounded observer
    /// leak across many visited profiles). [`Self::unregister_feed`] reads
    /// this map to tear the observer down alongside the feed controller and
    /// its snapshot projection. Protocol-agnostic teardown bookkeeping (a
    /// `key → id` map carries no NIP/kind knowledge — D0-clean).
    interest_feed_observers: Mutex<std::collections::HashMap<String, KernelEventObserverId>>,
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
    /// Test-only monotone send counter — counts every `send_cmd` call since
    /// construction, **never decremented**. Unlike `queue_depth` (which the
    /// actor thread races to decrement), this counter is a one-way ratchet that
    /// tests can use to assert "at least one command was enqueued" without a
    /// time-of-check / time-of-use race against the actor drain thread.
    ///
    /// Only compiled in `#[cfg(test)]`; zero overhead in production builds.
    #[cfg(test)]
    send_cmd_count: AtomicU64,
    /// Test-only last-command-variant tag — records the discriminant name of the
    /// most recently sent `ActorCommand` as a `'static str`. Unlike
    /// `send_cmd_count` (which only proves *something* was sent), this lets a
    /// test assert that the SPECIFIC variant that was expected was actually
    /// enqueued (e.g. `CancelPublish` vs. `RetryPublish`).
    ///
    /// Only compiled in `#[cfg(test)]`; zero overhead in production builds.
    #[cfg(test)]
    last_cmd_tag: std::sync::Mutex<Option<&'static str>>,
    /// D2 coverage-gate hook slot. Set by the per-app crate (`nmp-app-chirp`)
    /// via [`Self::set_coverage_hook`] before `nmp_app_start`. Actor startup
    /// snapshots it into immutable config, installs it on the
    /// `SubscriptionLifecycle`, and re-applies the same snapped hook after
    /// `Reset`. Kept in an `Arc<Mutex<Option<…>>>` slot so the per-app
    /// registration pattern mirrors `storage_path` and the other pre-start
    /// slots.
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
    /// Pre-start slot snapped when `nmp_app_start` consumes the actor starter:
    /// the per-app crate writes through this clone via
    /// [`Self::set_host_op_handler`] before start; the actor's `Protocol`
    /// dispatch arm reads the snapped handler (via the
    /// `HostOpHandlerAccess` capability) when an
    /// `ActionModule::execute` body enqueues a `HostOpCommand` (ADR-0052 §D4,
    /// K2 rung 5.4 — the bespoke `DispatchHostOp` arm was merged into
    /// `Protocol`). `None` (the default, and the only state for hosts that
    /// don't bind a stateful app) makes any such command record a `Failed`
    /// terminal stage — never a silent drop.
    host_op_handler: nmp_core::substrate::HostOpHandlerSlot,
    /// V-38: substrate-generic relay-text interceptor slot. A NIP-crate
    /// runtime (today `nmp-nip47`) installs itself here so the actor can
    /// peek at every inbound text frame and let the runtime decode
    /// protocol-specific responses (kind:23195 NWC). Shared `Arc` with the
    /// actor startup snapshots the installed interceptors into its relay-event
    /// config. Mutating the slot after `nmp_app_start` does not affect the
    /// running actor.
    relay_text_interceptor: nmp_core::substrate::RelayTextInterceptorSlot,
    /// ADR-0051 — relay-connected hook slot (twin of `relay_text_interceptor`
    /// above): `nmp-nip11` installs its fetch hook before start, and actor
    /// startup snapshots the installed hooks before fanning them on
    /// `PoolEvent::Opened`.
    relay_connected_hook: nmp_core::substrate::RelayConnectedHookSlot,
    /// ADR-0052 §D3 — per-app bunker-URI hook slot (replaces `bunker_hook::HOOK`;
    /// installed by `nmp_signer_broker_init`, read by the actor; dies with app).
    bunker_hook: nmp_core::BunkerHookSlot,
    /// ADR-0052 §D3 — per-app NIP-55 restore hook slot (replaces
    /// `external_signer_hook::HOOK`; installed by `nmp_external_signer_init`).
    external_signer_hook: nmp_core::ExternalSignerHookSlot,
    /// ADR-0052 §D3 — per-app NIP-46 broker handle (replaces `GLOBAL_BROKER`;
    /// `None` until `nmp_signer_broker_init`; read by cancel / nostrconnect-uri).
    #[cfg(feature = "signer-broker")]
    signer_broker: Arc<Mutex<Option<Arc<nmp_signer_broker::BunkerBroker>>>>,
    /// ADR-0052 §D3 — per-app NIP-55 driver handle (replaces `GLOBAL_DRIVER`;
    /// `None` until `nmp_external_signer_init`; read by signin / deliver).
    #[cfg(feature = "external-signer")]
    external_signer_driver: Arc<Mutex<Option<Arc<external_signer::Nip55Driver>>>>,
    /// V-40 — shared [`nmp_core::substrate::EventIngestDispatcher`] slot.
    /// Per-NIP crates register a parser through
    /// [`Self::register_ingest_parser`] which mutates this slot under a
    /// write lock; the actor's kernel construction binds the SAME `Arc`
    /// onto the kernel so the ingest path reads through the same
    /// dispatcher the registration path mutated.
    ingest_dispatcher_slot: Arc<std::sync::RwLock<nmp_core::substrate::EventIngestDispatcher>>,
    /// #1811 — shared crate-registered FTS scope registry. Per-protocol crates
    /// register a [`nmp_core::substrate::SearchScopeProvider`] through
    /// [`Self::register_search_scope`]; actor kernel construction compiles the
    /// registry into noun-free `CompiledIndexSpec`s and installs them into the
    /// kernel store (`apply_to_kernel`), so the SAME registry the registration
    /// path mutated drives the store's index.
    search_scope_registry: Arc<nmp_core::substrate::SearchScopeRegistry>,
    /// #1804 — shared crate-registered input-scope recognizer registry.
    /// Per-protocol / app crates register an
    /// [`nmp_core::substrate::InputScopeRecognizer`] through
    /// [`Self::register_input_scope`]; the input-intent resolver FFI reads a
    /// [`nmp_core::substrate::InputScopeRegistry::recognizers`] snapshot from
    /// this same handle to drive the pure `nmp_intent::classify` pass. Read on
    /// the FFI thread, never threaded through the actor (classify is pure / sync
    /// / IO-free; all IO happens in the dispatch layer).
    input_scope_registry: Arc<nmp_core::substrate::InputScopeRegistry>,
    /// V-40 — shared [`nmp_core::substrate::DmInboxRelayLookup`] slot. The
    /// per-app crate (today `nmp-nip17::register_actions`) writes a
    /// concrete `DmRelayCache` here via
    /// [`Self::set_dm_inbox_relay_lookup`]; actor startup snapshots the current
    /// handle and binds it onto the kernel so the gift-wrap publish path
    /// and the planner-side `KernelMailboxes` adapter both see the same
    /// cache.
    dm_inbox_relays_slot: Arc<Mutex<Arc<dyn nmp_core::substrate::DmInboxRelayLookup>>>,
    /// ADR-0057 PR 2 — shared [`nmp_core::substrate::ProfileLookup`] slot.
    /// Mirrors `dm_inbox_relays_slot`: the per-app crate writes a concrete
    /// `nmp_nip01::ProfileCache` here via [`Self::set_profile_lookup`]; the
    /// actor startup snapshots the current handle and binds it onto the kernel so the
    /// kernel's profile readers (enrichment, claim-TTL, zap LNURL, RAM
    /// eviction) read through the same `Arc` the kind:0 `Kind0Parser` writes.
    profile_lookup_slot: Arc<Mutex<Arc<dyn nmp_core::substrate::ProfileLookup>>>,
    /// ADR-0057 PR 3 — shared [`nmp_core::substrate::ContactsLookup`] slot.
    /// Mirrors `profile_lookup_slot`: the per-app crate writes a concrete
    /// `nmp_nip01::ContactsCache` here via [`Self::set_contacts_lookup`]; the
    /// actor startup snapshots the current handle and binds it onto the kernel so the
    /// kernel's follow-feed readers read through the same `Arc` the kind:3
    /// `Kind3Parser` writes.
    contacts_lookup_slot: Arc<Mutex<Arc<dyn nmp_core::substrate::ContactsLookup>>>,
    /// Substrate [`nmp_core::substrate::BlockedRelayLookup`] slot. Mirrors
    /// `dm_inbox_relays_slot`: the per-app crate (today: any app that
    /// wires `nmp_router::Kind10006Parser`) writes a concrete
    /// `Arc<InMemoryBlockedRelayCache>` here via
    /// [`Self::set_blocked_relay_lookup`]; actor startup snapshots the current
    /// handle and binds it onto the kernel so the kernel's
    /// `build_routing_context` snapshot helper reads through the same
    /// `Arc` the kind:10006 parser writes into.
    blocked_relays_slot: Arc<Mutex<Arc<dyn nmp_core::substrate::BlockedRelayLookup>>>,
    /// Per-app override for the active-account bootstrap Tailing
    /// self-kinds list. `None` (the default) makes the kernel use the
    /// built-in `[0, 3, 10002, 10006, 10007]` list at
    /// `active_account_bootstrap_requests`. Written by the per-app crate
    /// via [`Self::set_bootstrap_self_kinds`] before `nmp_app_start`;
    /// actor startup snapshots the resolved value and binds it onto the kernel via
    /// [`nmp_core::kernel::Kernel::set_bootstrap_self_kinds_override`].
    /// kind:10000 is intentionally absent — owned by `MuteRuntimeController`.
    bootstrap_self_kinds: Arc<Mutex<Option<Vec<u64>>>>,
    /// H4 — read-only [`nmp_core::substrate::MailboxCache`] handle used by the
    /// `nmp_app_encode_profile` NIP-19 identity encoder. The per-app crate
    /// (today `nmp-defaults`) writes the SAME `Arc<InMemoryMailboxCache>`
    /// here via [`Self::set_mailbox_cache_reader`] that it hands the routing
    /// factory and the `Kind10002Parser`. That instance identity is the whole
    /// ballgame: the encoder reads kind:10002 relay hints out of the very
    /// cache the parser fills on ingest, so it can prefer `nprofile` over a
    /// bare `npub`. A divergent / fresh instance silently kills the nprofile
    /// branch (the read always misses → always npub). Read-only — never
    /// shared with the actor thread, so unlike `dm_inbox_relays_slot` there is
    /// no `actor_*` clone. `None` (the default) means "no relay hints known",
    /// which the encoder treats as the npub fallback.
    mailbox_cache_reader: Mutex<Option<Arc<dyn nmp_core::substrate::MailboxCache>>>,
    /// NIP-50 higher-order search relay source (kind:10007 read seam +
    /// app-default fallback). The composition root installs a concrete
    /// `PreferredRelaySource` here via
    /// [`Self::install_preferred_relay_source`] (the `HostCapabilities` override)
    /// before `nmp_app_start`; `open_search` reads it to resolve `UserPreferred`
    /// / `AppDefault` targets. `None` (the default) makes those targets resolve
    /// to an empty relay set — a cache-only search rather than a crash (D6).
    /// Read-only on the FFI thread, never shared with the actor.
    search_relay_source: SearchRelaySourceSlot,
    /// Live NIP-50 search sessions, keyed by host session id. One record per
    /// [`Self::open_search`] call owning that session's teardown recipe (the
    /// muted observer id, the projection key, and the per-relay pinned-close
    /// args). [`Self::close_search`] looks the session up and runs its teardown
    /// idempotently. Protocol-agnostic bookkeeping (D0 — a `key → teardown` map
    /// carries no NIP nouns beyond the opaque interest args).
    search_sessions: Mutex<std::collections::HashMap<String, search::SearchSession>>,
    /// Test-support GC budget ceiling.  Set by `nmp_app_configure_gc_budget`
    /// before `nmp_app_start`; actor startup snapshots it into
    /// `ActorConfigSources::gc_budget_ceiling`.  `None` (the default) preserves
    /// `GcBudget::production()` (LRU deletion disabled).
    ///
    /// `Arc<Mutex<…>>` so the actor_starter closure can capture a clone and
    /// read the value at call time (after `nmp_app_configure_gc_budget` has
    /// written it). Mirrors the `kernel_clock` slot pattern.
    ///
    /// Only compiled under `cfg(any(test, feature = "test-support"))`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) gc_budget_ceiling: Arc<Mutex<Option<usize>>>,
}

#[no_mangle]
pub extern "C" fn nmp_app_new() -> *mut NmpApp {
    // ADR-0050 §D3a — one waking inbox of `ActorMail`. `command_tx` is the host
    // `CommandSender` (stored on `NmpApp`); the actor receives on `command_rx`.
    let (inbox_tx, command_rx) = mpsc::channel::<nmp_core::__ffi_internal::ActorMail>();
    let command_tx = nmp_core::CommandSender::new(inbox_tx);
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
    // Per-app idempotency slot — tracks the previously-installed singleton
    // kernel-event observer id for a per-app crate that wants exactly one
    // auxiliary `KernelEventObserver` per app. NOT shared with the actor
    // thread — the actor never reads this; only the FFI side calls the swap
    // accessor. Owned by `NmpApp`, dropped with it (so the slot dies with
    // the app — no global aliasing across `nmp_app_free`).
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
    // builds those itself and registers the typed `"wallet"` sidecar via
    // `register_typed_snapshot_projection` (ADR-0037) for the read side.
    // ADR-0052 rung 5.2: the write side is the per-app `WalletRuntimeHandle`
    // owned BY VALUE inside the wallet `ActionModule`s (no process-global
    // install). The actor now only carries a substrate-generic relay-text
    // interceptor slot.
    let relay_text_interceptor = nmp_core::substrate::new_relay_text_interceptor_slot();
    let actor_relay_text_interceptor = Arc::clone(&relay_text_interceptor);
    // ADR-0051: relay-connected hook slot (actor clone + `NmpApp` clone).
    let relay_connected_hook = nmp_core::substrate::new_relay_connected_hook_slot();
    let actor_relay_connected_hook = Arc::clone(&relay_connected_hook);
    // ADR-0052 §D3: per-app signer hook slots (replace the deleted
    // `bunker_hook::HOOK` / `external_signer_hook::HOOK` globals). The `NmpApp`
    // keeps one clone of each so the `*_init` symbols install post-construction;
    // the actor's `IdentityRuntime` carries the matching clone (sole reader).
    let bunker_hook = nmp_core::new_bunker_hook_slot();
    let actor_bunker_hook = Arc::clone(&bunker_hook);
    let external_signer_hook = nmp_core::new_external_signer_hook_slot();
    let actor_external_signer_hook = Arc::clone(&external_signer_hook);
    // D0: NIP-46 remote signing is an app noun. The shared bunker-handshake
    // slot is handed to the actor: `run_actor_with_observers` both gives one
    // `Arc` clone to the actor's `IdentityRuntime` (the sole writer, D4) and
    // registers the built-in `"bunker_handshake"` snapshot-projection closure
    // that reads the other clone. Handshake state therefore reaches the host
    // through `projections["bunker_handshake"]` instead of a baked-in
    // `KernelSnapshot` field — and every actor consumer (FFI or test) gets the
    // projection without a separate FFI registration step.
    let actor_bunker_handshake = new_bunker_handshake_slot();
    // ADR-0048 D6: unified remote-signer health slot. The broker callback
    // routes `BrokerEvent::ConnectionStateChanged` through
    // `ActorCommand::BunkerConnectionStateChanged` → actor → this slot → the
    // built-in `"signer_state"` snapshot projection. D4: the actor is the
    // sole writer of the slot.
    let actor_signer_state = new_signer_state_slot();
    // Shared relay-edit rows handle. Cloned to the actor thread and bound
    // onto the kernel so external Rust callers can read the user's current
    // relay list without crossing FFI.
    //
    // Typed slot constructor — see `kernel/relay_projection.rs`.
    let configured_relays: nmp_core::AppRelaySlot = new_app_relay_slot();
    let actor_configured_relays = Arc::clone(&configured_relays);
    // V-65 — host-supplied bootstrap relay for client-initiated NIP-46
    // `nostrconnect://` handshakes. Default `None`; the composition root
    // (e.g. `nmp_defaults::register_defaults`) writes a sane default via
    // `NmpApp::set_nostrconnect_bootstrap_relay` before `nmp_app_start`. Unlike
    // the relay-edit-rows slot, this is NOT shared with the actor thread
    // (the read path is FFI-synchronous on the calling thread).
    let nostrconnect_bootstrap_relay: NostrConnectBootstrapRelaySlot =
        new_nostrconnect_bootstrap_relay_slot();
    // #1493 P9 — host-supplied NIP-46 perm request for client-initiated
    // `nostrconnect://` handshakes. Default `None`; the leaf app (e.g. Chirp,
    // via `nmp_app_chirp_register`) writes its product policy via
    // `NmpApp::set_nostrconnect_perms` before `nmp_app_start`. NMP supplies no
    // default (#1493). Like the bootstrap-relay slot this is NOT shared with the
    // actor thread (the read path is FFI-synchronous on the calling thread).
    let nostrconnect_perms: NostrConnectPermsSlot = new_nostrconnect_perms_slot();
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
    // V-82 — active-account hex-pubkey slot. The `NmpApp` keeps one `Arc`
    // clone (read via `NmpApp::active_account_handle`); the actor carries the
    // matching clone and threads it INTO the kernel at construction (and
    // re-hands it on `Reset`) so the slot the kernel writes on every identity
    // mutation IS the slot the host reads — single source of truth.
    let active_account_handle: ActiveAccountSlot = new_active_account_slot();
    let actor_active_account = Arc::clone(&active_account_handle);
    let identity_change_observers = new_identity_change_observer_slot();
    let listener_identity_change_observers = Arc::clone(&identity_change_observers);
    let listener_active_account = Arc::clone(&active_account_handle);
    let listener_last_active_account = Arc::new(Mutex::new(None));
    // V-83 — event-store publish-back slot. The `NmpApp` keeps one `Arc` clone
    // (read via `NmpApp::event_by_id` / `event_store_handle`); the actor carries
    // the matching clone and publishes `kernel.event_store_handle()` into it
    // right after kernel construction (and re-publishes on `Reset`), so the
    // store the host reads IS the store the kernel writes — no divergent mirror.
    // Publish-back (kernel-built store), NOT the V-82 hand-down pattern.
    let event_store_handle: EventStoreSlot = new_event_store_slot();
    let actor_event_store = Arc::clone(&event_store_handle);
    // ADR-0058 step 3b — pull-cursor registry publish-back slot. The `NmpApp`
    // keeps one `Arc` clone (read by `nmp_app_pull_page`); the actor carries the
    // matching clone and publishes `kernel.pull_cursor_registry_handle()` into
    // it after kernel construction (and re-publishes on `Reset`).
    let pull_cursor_registry: PullCursorRegistryHandleSlot = new_pull_cursor_registry_handle_slot();
    let actor_pull_cursor_registry = Arc::clone(&pull_cursor_registry);
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
    // ADR-0049 Part 2 — the composition ledger, shared between the action
    // registry and this struct's wiring-slot recorders.
    let composition_ledger: Arc<nmp_core::CompositionLedger> =
        Arc::new(nmp_core::CompositionLedger::new());
    // Spec §271 (2026-05-25) — substrate-publish-resolver factory slot.
    // Default `None`; the per-app crate installs a closure via
    // `set_publish_resolver_factory` before `nmp_app_start`. The actor reads
    // the slot once after kernel construction (and once per `Reset`) and
    // applies the produced resolver via `Kernel::set_publish_resolver`.
    let publish_resolver: PublishResolverSlot = new_publish_resolver_slot();
    let actor_publish_resolver = Arc::clone(&publish_resolver);
    // Test-support kernel-clock injection slot. Default `None`; the kernel
    // keeps its `SystemClock`. Only the `#[cfg(test-support)]`
    // `NmpApp::set_kernel_clock_for_test` setter ever writes it. The actor
    // reads its clone once after kernel construction and applies it via
    // `Kernel::set_clock`.
    let kernel_clock: nmp_core::slots::KernelClockSlot = nmp_core::slots::new_kernel_clock_slot();
    let actor_kernel_clock = Arc::clone(&kernel_clock);
    let external_event_sink_policy: ExternalEventSinkPolicySlot =
        new_external_event_sink_policy_slot();
    let actor_external_event_sink_policy = Arc::clone(&external_event_sink_policy);
    let external_event_sink_dispatcher_slot = new_external_event_sink_dispatcher_slot();
    // Publish a constructed (but not-yet-bound) dispatcher into the slot NOW,
    // at app construction, so in-process relay-forwarding policies have a
    // registry to attach to. The actor adopts THIS instance and only calls
    // `bind_runtime` (spawns the worker) once the Pool exists.
    if let Ok(mut guard) = external_event_sink_dispatcher_slot.lock() {
        *guard = Some(nmp_core::substrate::ExternalEventSinkDispatcher::new());
    }
    let actor_external_event_sink_dispatcher_slot =
        Arc::clone(&external_event_sink_dispatcher_slot);
    let feed_registry = nmp_feed::new_feed_registry_slot();
    let feed_sessions = Arc::new(nmp_feed::FeedSessionRegistry::default());
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
    // [`NmpApp::set_coverage_hook`] before `nmp_app_start`; actor startup
    // snapshots the value and installs it on the `SubscriptionLifecycle`.
    // `Reset` re-applies the same snapped hook. `None` (the test default)
    // leaves the lifecycle's `coverage_hook: None` in place — every plan
    // flows straight to raw REQ, preserving legacy behaviour.
    let coverage_hook: Arc<Mutex<Option<PlanCoverageHook>>> = Arc::new(Mutex::new(None));
    let actor_coverage_hook = Arc::clone(&coverage_hook);
    let req_frame_interceptor = nmp_core::substrate::new_req_frame_interceptor_slot();
    let actor_req_frame_interceptor = Arc::clone(&req_frame_interceptor);
    // Substrate-generic host-op handler slot — actor startup snapshots this
    // into config, and the `Protocol` dispatch arm reaches that snapped handler
    // via the `HostOpHandlerAccess` capability when a `HostOpCommand` runs
    // (ADR-0052 §D4). The per-app crate (today
    // `nmp-app-marmot`) writes through `NmpApp::set_host_op_handler` before
    // `nmp_app_start`. `None` is the default and the production state for
    // every host that does not bind a stateful app crate.
    let host_op_handler: nmp_core::substrate::HostOpHandlerSlot =
        nmp_core::substrate::new_host_op_handler_slot();
    let actor_host_op_handler = nmp_core::substrate::HostOpHandlerSlot::clone(&host_op_handler);
    // V-40 — substrate `EventIngestDispatcher` slot. Per-NIP crates
    // (today: `nmp-nip17`) register their kind parsers through
    // [`NmpApp::register_ingest_parser`] which mutates THIS slot; the
    // actor startup binds it onto the kernel so the
    // ingest path and the registration path share one dispatcher.
    let ingest_dispatcher_slot: Arc<std::sync::RwLock<nmp_core::substrate::EventIngestDispatcher>> =
        Arc::new(std::sync::RwLock::new(
            nmp_core::substrate::EventIngestDispatcher::new(),
        ));
    let actor_ingest_dispatcher = Arc::clone(&ingest_dispatcher_slot);
    // #1811 — crate-registered FTS scope registry. Per-protocol crates register
    // a `SearchScopeProvider` through `NmpApp::register_search_scope` which
    // mutates THIS registry; the actor compiles + installs it into the kernel
    // store at construction so registration and the store index share one
    // registry.
    let search_scope_registry: Arc<nmp_core::substrate::SearchScopeRegistry> =
        Arc::new(nmp_core::substrate::SearchScopeRegistry::new());
    let actor_search_scope_registry = Arc::clone(&search_scope_registry);
    // #1804 — crate-registered input-scope recognizer registry. Per-protocol /
    // app crates register an `InputScopeRecognizer` through
    // `NmpApp::register_input_scope` which mutates THIS registry; the
    // input-intent resolver FFI reads a recognizers() snapshot from the same
    // handle to drive `nmp_intent::classify`. Not threaded through the actor
    // (classify is pure / IO-free).
    let input_scope_registry: Arc<nmp_core::substrate::InputScopeRegistry> =
        Arc::new(nmp_core::substrate::InputScopeRegistry::new());
    // V-40 — substrate `DmInboxRelayLookup` slot. The per-app crate
    // (today: `nmp-nip17::register_actions`) installs the concrete
    // `DmRelayCache` here via [`NmpApp::set_dm_inbox_relay_lookup`];
    // actor startup snapshots the current handle and binds
    // it onto the kernel. Default is `EmptyDmInboxRelayLookup` (fail-
    // closed cold-start).
    let dm_inbox_relays_slot: Arc<Mutex<Arc<dyn nmp_core::substrate::DmInboxRelayLookup>>> =
        Arc::new(Mutex::new(
            nmp_core::substrate::empty_dm_inbox_relay_lookup(),
        ));
    let actor_dm_inbox_relays = Arc::clone(&dm_inbox_relays_slot);
    // ADR-0057 PR 2 — substrate `ProfileLookup` slot. The per-app crate
    // (today: `nmp_defaults` register_substrate) installs the concrete
    // `nmp_nip01::ProfileCache` here via [`NmpApp::set_profile_lookup`]; the
    // actor startup snapshots the current handle and binds it onto
    // the kernel. Default is `EmptyProfileLookup` (cold-start, every lookup None).
    let profile_lookup_slot: Arc<Mutex<Arc<dyn nmp_core::substrate::ProfileLookup>>> =
        Arc::new(Mutex::new(nmp_core::substrate::empty_profile_lookup()));
    let actor_profile_lookup = Arc::clone(&profile_lookup_slot);
    // ADR-0057 PR 3 — substrate `ContactsLookup` slot. The per-app crate
    // (today: `nmp_defaults` register_substrate) installs the concrete
    // `nmp_nip01::ContactsCache` here via [`NmpApp::set_contacts_lookup`]; the
    // actor startup snapshots the current handle and binds it onto
    // the kernel. Default is `EmptyContactsLookup` (cold-start, every lookup None).
    let contacts_lookup_slot: Arc<Mutex<Arc<dyn nmp_core::substrate::ContactsLookup>>> =
        Arc::new(Mutex::new(nmp_core::substrate::empty_contacts_lookup()));
    let actor_contacts_lookup = Arc::clone(&contacts_lookup_slot);
    // Mirror dm_inbox_relays_slot for the blocked-relay lookup: empty
    // default until an app crate wires `nmp_router::InMemoryBlockedRelayCache`
    // through `set_blocked_relay_lookup`.
    let blocked_relays_slot: Arc<Mutex<Arc<dyn nmp_core::substrate::BlockedRelayLookup>>> =
        Arc::new(Mutex::new(nmp_core::substrate::empty_blocked_relay_lookup()));
    let actor_blocked_relays = Arc::clone(&blocked_relays_slot);
    // Per-app override for the bootstrap Tailing self-kinds list.
    // `None` → kernel uses its built-in default.
    let bootstrap_self_kinds: Arc<Mutex<Option<Vec<u64>>>> = Arc::new(Mutex::new(None));
    let actor_bootstrap_self_kinds = Arc::clone(&bootstrap_self_kinds);
    // Clone so we can report actor death through the same listener pipe.
    // The actor `move`s its own `update_tx` into `run_actor_with_observers`;
    // this clone is the supervisor's last live handle once that one is
    // dropped — it MUST outlive the inner closure so the panic frame can
    // still be delivered after the actor's own sender is gone.
    let update_tx_panic = update_tx.clone();
    let startup_update_tx = update_tx.clone();
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
    // Test-support GC budget ceiling slot. `nmp_app_configure_gc_budget` writes
    // through the NmpApp clone; the actor_starter closure captures a second clone
    // and reads it at call time (after pre-start config is applied). Pattern
    // mirrors `kernel_clock`.
    #[cfg(any(test, feature = "test-support"))]
    let gc_budget_ceiling: Arc<Mutex<Option<usize>>> = Arc::new(Mutex::new(None));
    #[cfg(any(test, feature = "test-support"))]
    let actor_gc_budget_ceiling = Arc::clone(&gc_budget_ceiling);

    let actor_command_tx_self = command_tx.clone();
    let actor_starter: ActorStarter = Box::new(move || {
        let channels = ActorChannels {
            inbox_rx: command_rx,
            command_tx_self: actor_command_tx_self,
            update_tx,
        };
        let runtime = ActorRuntimeSlots {
            lifecycle_observer: actor_lifecycle_observer,
            event_observers: actor_event_observers,
            snapshot_projections: actor_snapshot_projections,
            bunker_handshake: actor_bunker_handshake,
            signer_state: actor_signer_state,
            bunker_hook: actor_bunker_hook,
            external_signer_hook: actor_external_signer_hook,
            configured_relays: actor_configured_relays,
            mls_local_nsec: actor_mls_local_nsec,
            active_local_keys: actor_active_local_keys,
            capability_callback: actor_capability_callback,
            queue_depth: actor_queue_depth,
            routing_trace: actor_routing_trace,
            active_account: actor_active_account,
            event_store: actor_event_store,
            pull_cursor_registry: actor_pull_cursor_registry,
            external_event_sink_dispatcher: actor_external_event_sink_dispatcher_slot,
        };
        // Compute GC budget ceiling at start time (after nmp_app_configure_gc_budget).
        // In non-test-support builds actor_gc_budget_ceiling doesn't exist, so this
        // falls back to None (production default = LRU disabled).
        #[cfg(any(test, feature = "test-support"))]
        let gc_budget_ceiling_for_config: Option<usize> =
            actor_gc_budget_ceiling.lock().ok().and_then(|g| *g);
        #[cfg(not(any(test, feature = "test-support")))]
        let gc_budget_ceiling_for_config: Option<usize> = None;

        let config = ActorConfigSources {
            storage_path: actor_storage_path,
            coverage_hook: actor_coverage_hook,
            req_frame_interceptor: actor_req_frame_interceptor,
            host_op_handler: actor_host_op_handler,
            relay_text_interceptor: actor_relay_text_interceptor,
            relay_connected_hook: actor_relay_connected_hook,
            ingest_dispatcher: actor_ingest_dispatcher,
            search_scope_registry: actor_search_scope_registry,
            dm_inbox_relays: actor_dm_inbox_relays,
            profile_lookup: actor_profile_lookup,
            contacts_lookup: actor_contacts_lookup,
            blocked_relays: actor_blocked_relays,
            bootstrap_self_kinds: actor_bootstrap_self_kinds,
            routing_substrate: actor_routing_substrate,
            publish_resolver: actor_publish_resolver,
            external_event_sink_policy: actor_external_event_sink_policy,
            kernel_clock: actor_kernel_clock,
            gc_budget_ceiling: gc_budget_ceiling_for_config,
        }
        .snapshot();
        thread::spawn(move || {
            // D7 (actor-death visibility): the actor thread owns the kernel loop.
            // If it panics, `send_cmd` would otherwise silently drop every
            // subsequent command (the channel closes with no signal). Catch the
            // unwind here and emit one envelope-conforming `Panic` frame on the
            // update channel *before* this thread (and `update_tx`) is dropped,
            // so the host receives a terminal, decodable signal.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_actor_with_observers(channels, config, runtime);
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
        })
    });
    let _ = startup_update_tx.send(prestart_snapshot_frame(0));
    let (embed_sidecar, listener_embed_sidecar) = snapshot::embed_sidecar::new_embed_sidecar_pair();
    let update_listener = thread::spawn(move || {
        while let Ok(update) = update_rx.recv() {
            snapshot::embed_sidecar::update_embed_sidecar_from_frame(
                &update,
                &listener_embed_sidecar,
            );
            notify_identity_change_observers(
                &listener_active_account,
                &listener_last_active_account,
                &listener_identity_change_observers,
            );
            // Quiescence-safe callback invocation (option b — Condvar drain).
            //
            // 1. Lock the gate, copy the registration, increment `in_flight`
            //    while still holding the lock. Incrementing under lock is the
            //    critical ordering: it ensures that a concurrent
            //    `nmp_app_set_update_callback` that is waiting for `in_flight
            //    == 0` cannot sneak past before we have committed to one more
            //    invocation.
            // 2. Drop the lock. The foreign callback runs outside the mutex so
            //    that the setter never holds a Rust lock while foreign code
            //    executes (avoids deadlock if a host re-enters another NMP FFI
            //    entry point, though the current hosts do not).
            // 3. After the callback returns, re-acquire the lock, decrement
            //    `in_flight`, and signal `drained` so a waiting setter can
            //    make progress.
            let registration = {
                // D6 fail-loud: a poisoned lock must NOT silently skip
                // delivering the update frame to the host — that freezes the
                // UI. The inner gate state stays structurally valid across a
                // panic (it is plain counters + a registration handle), so we
                // recover the guard and keep firing rather than `continue`.
                let mut guard = listener_callback.inner.lock().unwrap_or_else(|e| {
                    tracing::error!("listener lock was poisoned; recovering");
                    e.into_inner()
                });
                let reg = guard.registration;
                if reg.is_some() {
                    guard.in_flight += 1;
                }
                reg
            };
            if let Some(registration) = registration {
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
                // Decrement in_flight and wake any waiting setter. Recover from
                // poison here too: a leaked `in_flight` would block a quiescing
                // setter forever (another silent freeze), so we must always
                // balance the increment above.
                let mut guard = listener_callback.inner.lock().unwrap_or_else(|e| {
                    tracing::error!("listener lock was poisoned; recovering");
                    e.into_inner()
                });
                guard.in_flight = guard.in_flight.saturating_sub(1);
                if guard.in_flight == 0 {
                    listener_callback.drained.notify_all();
                }
            }
        }
    });
    let app = NmpApp {
        tx: command_tx,
        update_callback,
        identity_change_observers,
        capability_callback,
        lifecycle_observer,
        event_observers,
        singleton_event_observer_id,
        configured_relays,
        initial_relays_for_start: Mutex::new(Vec::new()),
        nostrconnect_bootstrap_relay,
        nostrconnect_perms,
        mls_local_nsec,
        active_local_keys,
        active_account_handle,
        event_store_handle,
        pull_cursor_registry,
        storage_path,
        routing_trace,
        routing_substrate,
        publish_resolver,
        kernel_clock,
        external_event_sink_policy,
        pending_mls_autopublish,
        actor_starter: Mutex::new(Some(actor_starter)),
        startup_update_tx: Mutex::new(Some(startup_update_tx)),
        actor: Mutex::new(None),
        update_listener: Mutex::new(Some(update_listener)),
        // M6 — the action registry the kernel ships with: `PublishModule`
        // only. NIP-29 / NIP-59 modules are app nouns (D0) and are
        // registered by the app host against its own registry instance.
        //
        // ADR-0049: the registry carries the shared composition ledger so every
        // `register` / `register_default` decision is recorded. `default_registry()`
        // installs `PublishModule` via the bare `register` path before the ledger
        // is attached — that single seeded entry is the kernel's own and is not
        // ledger-recorded (it is constant across every app); all host-init
        // registrations after this point ARE recorded.
        action_registry: default_registry()
            .with_composition_ledger(Arc::clone(&composition_ledger)),
        composition_ledger,
        // ADR-0049 Part 2 — not started until `nmp_app_start` sends Start.
        started: AtomicBool::new(false),
        // Host-extensible snapshot output: ships with the built-in `"wallet"`
        // projection (registered below when `feature = "wallet"`). A non-social
        // host registers its own projections via
        // `nmp_app_register_snapshot_projection` during init.
        snapshot_projections,
        feed_registry,
        // #1740 step 2 — empty until the first `open_feed`.
        feed_sessions,
        // #1740 step 4 — empty until the first `register_custom_perspective`.
        custom_perspectives: Arc::new(nmp_feed::PerspectiveRegistry::default()),
        // Per-open transient-feed observer bookkeeping; empty until the first
        // `register_feed_with_observer` (a visited profile / open thread).
        interest_feed_observers: Mutex::new(std::collections::HashMap::new()),
        // G-S4 — the `NmpApp`'s clone of the command-channel depth counter,
        // incremented by `send_cmd`. The actor holds the matching clone.
        queue_depth,
        // Test-only monotone send counter — starts at zero; incremented by
        // every `send_cmd` call and never decremented. Initialised here so
        // the `cfg(test)` field is present from construction.
        #[cfg(test)]
        send_cmd_count: AtomicU64::new(0),
        #[cfg(test)]
        last_cmd_tag: std::sync::Mutex::new(None),
        // D2 — the `NmpApp`'s clone of the coverage-gate hook slot. Written
        // by the per-app crate via [`NmpApp::set_coverage_hook`] before
        // `nmp_app_start`; actor startup snapshots it into config and
        // installs the hook on `SubscriptionLifecycle`.
        coverage_hook,
        req_frame_interceptor,
        // The `NmpApp`'s clone of the host-op handler slot. Written by the
        // per-app crate (today `nmp-app-marmot`) via
        // [`NmpApp::set_host_op_handler`] before `nmp_app_start`; actor
        // startup snapshots it for the `Protocol` arm (ADR-0052 §D4).
        host_op_handler,
        // V-38: pre-start relay-frame hooks; actor startup snapshots them into
        // relay-event config.
        relay_text_interceptor,
        relay_connected_hook,
        // ADR-0052 §D3 — per-app signer hook slots + broker/driver handles
        // (replace `GLOBAL_BROKER` / `GLOBAL_DRIVER` / the `HOOK` statics).
        // Broker/driver start `None`; written by the `*_init` symbols.
        bunker_hook,
        external_signer_hook,
        #[cfg(feature = "signer-broker")]
        signer_broker: Arc::new(Mutex::new(None)),
        #[cfg(feature = "external-signer")]
        external_signer_driver: Arc::new(Mutex::new(None)),
        // V-40 — substrate ingest/lookup wiring. The dispatcher remains a
        // shared registry handle; lookup slots are snapped to their current
        // handles at actor start.
        ingest_dispatcher_slot,
        // #1811 — crate-registered FTS scope registry (shared handle; compiled
        // + installed into the kernel store at actor construction).
        search_scope_registry,
        // #1804 — crate-registered input-scope recognizer registry (shared
        // handle read by the input-intent resolver FFI; not actor-threaded).
        input_scope_registry,
        dm_inbox_relays_slot,
        // ADR-0057 PR 2 — the `NmpApp`'s clone of the substrate `ProfileLookup`
        // slot. Mirrors the dm_inbox_relays_slot wiring.
        profile_lookup_slot,
        // ADR-0057 PR 3 — the `NmpApp`'s clone of the substrate `ContactsLookup`
        // slot. Mirrors the profile_lookup_slot wiring.
        contacts_lookup_slot,
        // Blocked-relay lookup + reactive-bootstrap self-kinds override
        // slots. Mirrors the dm_inbox_relays_slot wiring above.
        blocked_relays_slot,
        bootstrap_self_kinds,
        // H4 — read-only NIP-19 encoder cache handle. Empty until an app
        // crate wires the SAME `InMemoryMailboxCache` it gives the routing
        // factory + Kind10002Parser through `set_mailbox_cache_reader`.
        mailbox_cache_reader: Mutex::new(None),
        // NIP-50 search relay source — None until the composition root installs
        // the kind:10007 read seam + app-default via `install_preferred_relay_source`.
        search_relay_source: new_search_relay_source_slot(),
        // NIP-50 live search sessions — empty until the first open_search.
        search_sessions: Mutex::new(std::collections::HashMap::new()),
        // Test-support GC budget ceiling — None (production default = LRU disabled)
        // until `nmp_app_configure_gc_budget` is called before start.
        #[cfg(any(test, feature = "test-support"))]
        gc_budget_ceiling,
    };
    // V-38: the `"wallet"` snapshot projection moved to `crates/nmp-nip47`.
    // The host (per-app crate) registers it themselves on the `NmpApp` as a
    // typed `"wallet"` sidecar via `register_typed_snapshot_projection`
    // (ADR-0037) after constructing the `WalletStatusSlot` from `nmp_nip47`.
    //
    // D0 — the built-in `"bunker_handshake"` projection is registered inside
    // `run_actor_with_observers` (at the actor wiring site), not here: it
    // reads the actor-owned bunker-handshake slot, so every actor consumer
    // (FFI or test) gets the projection without a separate FFI step.
    snapshot::embed_sidecar::install_embed_sidecar_projection(&app, embed_sidecar); // #1283.

    // Issue #1238: install the per-app NIP-55 restore hook before any host can
    // send `Start`. The driver does not need the Android capability callback
    // for pubkey-only restore; it reads that shared slot later when an op
    // dispatches.
    #[cfg(feature = "external-signer")]
    external_signer::init_external_signer_driver(&app);
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
        // Test-only monotone counter: never decremented, so tests can assert
        // "at least one command was sent" without racing the actor drain thread
        // (the TOCTOU race that made `queue_depth` unreliable for that use).
        #[cfg(test)]
        self.send_cmd_count.fetch_add(1, Ordering::Relaxed);
        // Test-only last-variant tag: records which `ActorCommand` was most
        // recently sent, so tests can assert the SPECIFIC variant (e.g.
        // `CancelPublish`, not just "some command") without inspecting the actor's
        // internal state. Only the discriminant names needed by existing tests are
        // listed; the `_` arm covers all others.
        #[cfg(test)]
        if let Ok(mut tag) = self.last_cmd_tag.lock() {
            *tag = Some(match &cmd {
                ActorCommand::CancelPublish { .. } => "CancelPublish",
                ActorCommand::RetryPublish { .. } => "RetryPublish",
                _ => "_other",
            });
        }
        let _ = self.tx.send(cmd);
    }

    /// Declare a feed of app-owned primary kinds from the active account's
    /// reactive follows perspective.
    ///
    /// The caller supplies primary content kinds only. Repost wrappers are
    /// derived here before the actor receives the compiled acquisition set, so
    /// `nmp-core` never owns the app's primary-kind policy. A wrapper kind
    /// supplied as a primary kind is rejected and surfaced as state (toast),
    /// matching the C-ABI helper's D6 behavior.
    pub fn declare_active_follows_feed<I>(&self, primary_kinds: I) -> bool
    where
        I: IntoIterator<Item = u32>,
    {
        let acquisition_kinds = match nmp_nip18::try_acquisition_kinds_for_primary(primary_kinds) {
            Ok(kinds) => kinds,
            Err(_) => {
                self.send_cmd(ActorCommand::ShowToast {
                    message:
                        "declare_active_follows_feed: primary kinds must not include repost wrappers or the delete kind"
                            .to_string(),
                });
                return false;
            }
        };
        self.send_cmd(ActorCommand::DeclareActiveFollowsFeed { acquisition_kinds });
        true
    }

    /// Clear the active-follows feed declaration.
    pub fn clear_active_follows_feed(&self) {
        self.send_cmd(ActorCommand::ClearActiveFollowsFeed);
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
    /// and before any [`action::nmp_app_dispatch_action_bytes`] call. ADR-0052 rung
    /// 5.2: takes the module **value** so a stateful module (e.g. one owning
    /// an `Arc<WalletRuntimeHandle>`) carries its deps, captured at
    /// composition time, instead of reaching a process-global.
    pub fn register_action<M: nmp_core::substrate::ActionModule + 'static>(&mut self, module: M) {
        self.action_registry.register(module);
    }

    /// Register a typed action module as a **yielding default** (ADR-0049
    /// Part 1): install it only if its namespace is unclaimed; otherwise yield
    /// to the existing registration regardless of call order. Returns `true`
    /// when installed, `false` when it yielded. The canonical NMP defaults
    /// (`nmp_nip02` / `nmp_nip17` / `nmp_nip57` / `nmp_router`) register through
    /// this path. ADR-0052 rung 5.2: takes the module **value**.
    pub fn register_default_action<M: nmp_core::substrate::ActionModule + 'static>(
        &mut self,
        module: M,
    ) -> bool {
        self.action_registry.register_default(module)
    }

    /// Typed-only byte-doorway gate probe (ADR-0064 / #1756): the namespaces of
    /// every registered action module that is NOT typed-capable — i.e. that
    /// would be rejected `NotTypedCapable` by the byte doorway
    /// (`nmp_app_dispatch_action_bytes`) because it left
    /// [`nmp_core::substrate::ActionModule::decode_payload`] defaulted (a
    /// JSON-only / no-decode_payload module). The byte doorway is typed-only;
    /// this exposes the registry's intrinsic
    /// [`ActionRegistry::untyped_namespaces`](nmp_core::kernel::ActionRegistry::untyped_namespaces)
    /// so a composition gate (e.g. `nmp-defaults` after `register_defaults`) can
    /// assert the full production module set is typed and never re-grows a
    /// JSON-compat shim. Test-only surface — not a stable FFI/ABI boundary.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn untyped_action_namespaces(&self) -> Vec<String> {
        self.action_registry.untyped_namespaces()
    }

    /// ADR-0049 — read-only handle to the composition ledger for
    /// `nmp_app_composition_report`.
    #[must_use]
    pub fn composition_ledger(&self) -> &Arc<nmp_core::CompositionLedger> {
        &self.composition_ledger
    }

    /// ADR-0049 Part 2 — record a last-writer-wins **wiring-slot** decision.
    ///
    /// `seam`/`key` name the slot (e.g. `"routing_substrate"`). When the app is
    /// already started the value is dropped by the actor (it read the slot once
    /// at kernel construction), so this records [`nmp_core::Disposition::DroppedLateWiring`];
    /// otherwise the slot is being (re)written pre-start. `had_previous` is
    /// `true` when the slot already held a value — distinguishing a first
    /// install from an overwrite.
    pub(crate) fn record_slot_decision(
        &self,
        seam: &'static str,
        key: &'static str,
        had_previous: bool,
    ) {
        let disposition = if self.started.load(Ordering::SeqCst) {
            nmp_core::Disposition::DroppedLateWiring
        } else if had_previous {
            nmp_core::Disposition::ReplacedPrevious
        } else {
            nmp_core::Disposition::Installed
        };
        self.composition_ledger
            .record(seam, key, key, disposition, None);
    }

    /// Register a host-supplied snapshot projection — the output-side
    /// counterpart to [`Self::register_action`].
    ///
    /// The closure runs on **every snapshot tick** (inside the actor's
    /// `make_update`) and its returned JSON value is appended to
    /// `KernelSnapshot::projections` under `key`. A marketplace app registers
    /// `"market.listings"`, a todo app registers `"todo.items"` — each gets
    /// its own snapshot namespace WITHOUT editing `nmp-core`'s sealed social

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
    }

    #[must_use]
    pub fn load_older_feed(&self, key: &str) -> bool {
        let changed = self.feed_registry.load_older(key);
        if changed {
            self.send_cmd(ActorCommand::MarkChangedSinceEmit);
        }
        changed
    }

    /// Register a **transient** feed surface — a feed whose snapshot key must
    /// be torn down when its screen closes (a visited profile / open thread),
    /// as opposed to [`Self::register_feed`]'s permanent feeds (the home
    /// feed).
    ///
    /// This does everything `register_feed` does — registers the
    /// [`nmp_feed::FeedController`] under `key` in the feed registry (the
    /// render payload is emitted by a separately-registered typed snapshot
    /// projection, e.g. `register_typed_feed_sidecar`, not by this call) — AND
    /// additionally installs `observer` into the kernel's
    /// [`KernelEventObserver`] registry in **muted** state (ADR-0062).  The
    /// observer will NOT fire from the global fan-out until the caller passes
    /// the returned id to [`Self::open_observed_interest`], which replays the
    /// in-memory read-cache (and, for explicit `ids`-bearing shapes, the durable
    /// store) to the observer and then activates it.  The caller typically
    /// passes the same `Arc<FlatFeed>` as both `controller` and `observer`.
    ///
    /// Registering the same `key` twice replaces the controller / projection
    /// (last-writer-wins) and revokes the previously-tracked observer before
    /// installing the new one, so a re-open never leaks the prior observer.
    ///
    /// D6 — a poisoned bookkeeping mutex degrades to "observer registered but
    /// untracked": the feed still works, but its observer outlives the screen
    /// (a bounded soft-leak, never a crash). D8 — init-style registry push.
    #[must_use = "pass the returned id to open_observed_interest for catch-up"]
    pub fn register_feed_with_observer(
        &self,
        key: impl Into<String>,
        controller: std::sync::Arc<dyn nmp_feed::FeedController>,
        observer: std::sync::Arc<dyn KernelEventObserver>,
    ) -> KernelEventObserverId {
        let key = key.into();
        self.register_feed(key.clone(), controller);
        // ADR-0062: register muted so the observer doesn't receive events
        // from the global fan-out until the replay+activate step completes.
        let observer_id = register_rust_observer_muted(&self.event_observers, observer);
        if let Ok(mut map) = self.interest_feed_observers.lock() {
            if let Some(previous) = map.insert(key, observer_id) {
                // A re-open under the same key: the new observer is now
                // tracked; revoke the stale one so the kernel stops fanning
                // events into the replaced feed instance.
                self.unregister_event_observer(previous);
            }
        }
        observer_id
    }

    /// ADR-0062 — open an interest with read-model catch-up replay to the
    /// muted observer identified by `observer_id`, then activate it.
    ///
    /// Validates the filter JSON via `InterestShape::from_filter_json` and
    /// sends `ActorCommand::OpenObservedInterest`. A malformed filter emits a
    /// toast (same as `nmp_app_open_interest`) and returns without sending.
    ///
    /// `replay_shapes` are the `InterestShape`s used to match events in the
    /// kernel's read-cache during replay. These may differ from the filter
    /// (e.g. a thread feed uses two shapes: `#e` replies + root-by-id).
    pub fn open_observed_interest(
        &self,
        filter_json: &str,
        consumer_id: &str,
        scope: u32,
        observer_id: KernelEventObserverId,
        replay_shapes: Vec<nmp_planner::InterestShape>,
        replay_limit: usize,
    ) {
        self.open_observed_interest_pinned(
            filter_json,
            consumer_id,
            scope,
            None,
            observer_id,
            replay_shapes,
            replay_limit,
        );
    }

    /// ADR-0062 + relay-pin — the [`Self::open_observed_interest`] variant that
    /// routes the interest to exactly one relay (the planner's relay-pin lane).
    ///
    /// `relay_pin` — `Some(host)` pins the interest to that relay, bypassing
    /// NIP-65 outbox routing; `None` is identical to `open_observed_interest`.
    /// NIP-50 search (`nmp_app_search_open`) opens one pinned interest per
    /// resolved search relay. The pin participates in the `InterestShape` hash,
    /// so the matching close MUST pass the same pin (see
    /// [`Self::close_interest_pinned`]).
    #[allow(clippy::too_many_arguments)]
    pub fn open_observed_interest_pinned(
        &self,
        filter_json: &str,
        consumer_id: &str,
        scope: u32,
        relay_pin: Option<String>,
        observer_id: KernelEventObserverId,
        replay_shapes: Vec<nmp_planner::InterestShape>,
        replay_limit: usize,
    ) {
        // Validate filter — same guard as nmp_app_open_interest.
        if nmp_planner::InterestShape::from_filter_json(filter_json).is_none() {
            // D6: invalid filter is a no-op (the caller already surfaced a
            // toast via the validated FFI path; this internal helper does not
            // cross C ABI so we can stay silent here).
            return;
        }
        self.send_cmd(ActorCommand::OpenObservedInterest {
            filter_json: filter_json.to_string(),
            consumer_id: consumer_id.to_string(),
            scope,
            relay_pin,
            observer_id,
            replay_shapes,
            replay_limit,
        });
    }

    /// Send a relay-pinned `CloseInterest` matching a
    /// [`Self::open_observed_interest_pinned`] open. The `(filter_json,
    /// consumer_id, scope, relay_pin)` tuple MUST match the open so the
    /// reconstructed `InterestShape` hash lands on the same registry slot.
    pub fn close_interest_pinned(
        &self,
        filter_json: &str,
        consumer_id: &str,
        scope: u32,
        relay_pin: Option<String>,
    ) {
        self.send_cmd(ActorCommand::CloseInterest {
            filter_json: filter_json.to_string(),
            consumer_id: consumer_id.to_string(),
            scope,
            relay_pin,
        });
    }

    /// Tear down a feed registered through [`Self::register_feed_with_observer`].
    ///
    /// Performs all three removals the registration installed, in any
    /// combination present (each is an independent no-op when its target is
    /// absent, so an unknown key is harmless):
    ///
    /// 1. the [`nmp_feed::FeedController`] from the feed registry;
    /// 2. the snapshot projection closure (generic + typed) so it stops
    ///    emitting a stale empty subtree on every tick;
    /// 3. the tracked [`KernelEventObserver`], if one was recorded for `key`.
    ///
    /// CALLER CONTRACT — call this ONLY for transient keys registered through
    /// [`Self::register_feed_with_observer`]. It is **destructive on any key**
    /// that has a live `FeedController` / projection: calling it on the
    /// permanent home-feed key (`nmp.feed.home`, registered via the plain
    /// [`Self::register_feed`]) WOULD drop the home feed's controller and
    /// projection — it is "safe" there only in the sense that it never panics,
    /// not that it preserves the feed. The home feed has no tracked observer, so
    /// step 3 is a no-op there, but steps 1–2 are not.
    ///
    /// Returns `true` when any registration was removed. D6 — poisoned locks
    /// degrade to partial teardown (best-effort); the `nmp_app_free` actor
    /// join remains the hard fence for in-flight callbacks.
    pub fn unregister_feed(&self, key: &str) -> bool {
        let removed_feed = self.feed_registry.unregister(key);
        // ADR-0063 D7 (#1671 Lane H) — remove this feed's typed projection AND
        // its feed-author provider in the same lock. Dropping the provider means
        // the kernel's next in-tick reconcile no longer sees this consumer in
        // the live set and releases-all the refs it auto-resolved (the transient
        // author/thread feed leak guard; the permanent home feed keeps its
        // provider and is never swept).
        let removed_projection = self
            .snapshot_projections
            .lock()
            .map(|mut registry| {
                let removed_proj = registry.remove(key);
                let removed_provider = registry.remove_feed_author_provider(key);
                removed_proj || removed_provider
            })
            .unwrap_or(false);
        let removed_observer = self
            .interest_feed_observers
            .lock()
            .ok()
            .and_then(|mut map| map.remove(key));
        let removed_any = removed_feed || removed_projection || removed_observer.is_some();
        if let Some(observer_id) = removed_observer {
            self.unregister_event_observer(observer_id);
        }
        if removed_any {
            self.send_cmd(ActorCommand::MarkChangedSinceEmit);
        }
        removed_any
    }

    /// Register a host-supplied action-result observer — the *push*
    /// counterpart to [`Self::register_snapshot_projection`]'s pull seam.
    ///
    /// After [`action::nmp_app_dispatch_action_bytes`] accepts an action and its
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

    /// Test-only: run every registered **typed** snapshot projection directly
    /// against the app's shared registry, bypassing the actor/kernel tick. The
    /// typed counterpart to [`Self::run_snapshot_projections_for_test`] — lets
    /// the FFI registration tests assert that a projection registered through
    /// the `AppHost::register_typed_snapshot_projection` trait seam surfaces in
    /// the typed sidecar (`run_typed`).
    #[cfg(test)]
    pub(crate) fn run_typed_snapshot_projections_for_test(
        &self,
    ) -> Vec<nmp_core::TypedProjectionData> {
        self.snapshot_projections
            .lock()
            .map(|mut registry| registry.run_typed())
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
        // #1676: `ActionRegistry::execute` now returns a typed
        // `ActionExecuteFailure`; this test seam keeps its `String` surface by
        // flattening to the failure message.
        self.action_registry
            .execute(namespace, action_json, "test-correlation-id", &|cmd| {
                self.send_cmd(cmd)
            })
            .map_err(|failure| failure.message)
    }

    /// Set the one-shot MLS-autopublish intent (consumed by
    /// [`Self::take_pending_mls_autopublish`] in `register_with_keys`).
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
    pub fn actor_sender(&self) -> nmp_core::CommandSender {
        self.tx.clone()
    }

    /// Add a signer through the actor-owned identity reducer — the **single
    /// documented entry point** for all sign-in paths.
    ///
    /// This method replaced the legacy `sign_in_nsec` / `sign_in_bunker` /
    /// `add_remote_signer` surface. All sign-in paths ultimately funnel here:
    ///
    /// | Caller | Source | Notes |
    /// |--------|--------|-------|
    /// | `nmp_app_signin_nsec` (C-ABI) | `LocalNsec` | Shells without Marmot; `make_active` is caller-controlled |
    /// | `nmp_app_signin_bunker` (C-ABI) | `BunkerUri` | NIP-46 remote signer; `make_active` stashed across handshake |
    /// | `nmp-marmot::identity::sign_in_nsec_with_keyring_account` | `LocalNsec` | Persists secret to keyring first, then calls here |
    /// | `nmp-marmot::identity::restore_identity_with_keyring_account` | `LocalNsec` | Recalls secret from keyring, then calls here |
    /// | `nmp_app_signin_nip55` (C-ABI) | `RemoteHandle` | NIP-55 external signer; resolves via `Nip55Connect` |
    ///
    /// Session persistence is actor-owned for active signers and app-managed
    /// local signer slots. `add_signer` only enqueues `ActorCommand::AddSigner`
    /// and arms the MLS autopublish flag for active local-key sign-ins.
    ///
    /// `make_active` activates the resulting account once it resolves (for a
    /// `BunkerUri` source the flag is stashed across the async handshake
    /// round-trip; see [`nmp_core::SignerSource`]). PR-4 (D4 — one place): an
    /// active local-key sign-in is the single fact that arms MLS autopublish,
    /// consumed by `nmp_marmot_register[_active]`; bunker/non-active do not.
    pub fn add_signer(&self, source: nmp_core::SignerSource, make_active: bool) {
        if make_active && matches!(source, nmp_core::SignerSource::LocalNsec(_)) {
            self.set_pending_mls_autopublish(true);
        }
        self.send_cmd(ActorCommand::AddSigner {
            source,
            make_active,
        });
    }

    /// Remove an identity through the actor-owned identity reducer.
    pub fn remove_account(&self, identity_id: String) {
        self.send_cmd(ActorCommand::RemoveAccount { identity_id });
    }

    // `remove_account_forgetting_keyring` lives in `keyring_forget.rs` (kept out
    // of this file to respect its LOC ceiling — the D6 fail-loud body is larger
    // than the one-liner it replaced).

    /// Recall a previously-persisted local secret from the keyring capability.
    ///
    /// Returns `None` if the keyring reports `NotFound` or `Error` (no secret
    /// stored for this `account_id`, or the capability is unavailable).
    ///
    /// This is the low-level keyring-recall primitive consumed by
    /// `nmp-marmot::identity` — keyring orchestration (persist → sign-in,
    /// recall → sign-in) lives in that crate, not here.
    pub fn recall_local_nsec(&self, account_id: &str) -> Option<String> {
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

    /// Remove a typed snapshot projection registered under `key`.
    ///
    /// Idempotent: an unknown key is a silent no-op (D6). Signals a
    /// `MarkChangedSinceEmit` so the next snapshot tick reflects the removal
    /// (the stale projection stops emitting its subtree).
    pub fn remove_snapshot_projection(&self, key: &str) {
        let removed = self
            .snapshot_projections
            .lock()
            .map(|mut registry| registry.remove(key))
            .unwrap_or(false);
        if removed {
            self.send_cmd(ActorCommand::MarkChangedSinceEmit);
        }
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

    /// Atomically swap the per-app's singleton kernel-event observer-id slot:
    /// store `new` and return whatever was previously installed there.
    ///
    /// Idempotent-re-invoke contract: a per-app crate that wires exactly one
    /// auxiliary `KernelEventObserver` per app uses this slot to ensure a
    /// second registration unregisters the first one before installing itself.
    /// A poisoned mutex degrades to `None` (D6).
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
    /// registered `IngestParser` seams automatically, with no Swift polling needed.
    pub fn push_interest(&self, interest: nmp_planner::LogicalInterest) {
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

    /// V-82 — clone of the kernel's active-account hex-pubkey slot (`Arc`).
    ///
    /// Returns the SAME `Arc<Mutex<Option<String>>>` the kernel actor writes
    /// on every identity mutation (sign-in, account-switch, logout). The
    /// `NmpApp` constructs the slot at `nmp_app_new` and hands the kernel an
    /// `Arc::clone` at actor startup (re-handed across `Reset`), so a value
    /// read through the returned handle reflects the live active account —
    /// not a copy, not a mirror. `None` means no account is signed in.
    ///
    /// This is the V-80 OP-feed read seam: the composition root reads this
    /// handle for `nmp_nip02::ActiveFollowSet::new`. Identity-change push
    /// notification is provided separately by
    /// [`Self::register_identity_change_observer`].
    ///
    /// Substrate-generic — the slot holds a raw pubkey `String`; what callers
    /// do with it is their concern (D0). Parallel in shape to
    /// [`Self::active_local_keys`].
    #[must_use]
    pub fn active_account_handle(&self) -> ActiveAccountSlot {
        Arc::clone(&self.active_account_handle)
    }

    /// Register a Rust-side callback for active-account changes.
    ///
    /// The callback runs on the update-listener thread after the actor has
    /// written [`Self::active_account_handle`] and emitted an update frame. It
    /// fires only when the slot value changes (`Some(pubkey)` on sign-in/switch,
    /// `None` on logout/reset), never on ordinary snapshot ticks. This is the
    /// canonical app/FFI composition seam for long-lived Rust projections that
    /// need to reset per-account state without polling the slot.
    ///
    /// No unregister is provided because the current consumers are app-lifetime
    /// registrations installed during host init, matching permanent home-feed
    /// observer registration.
    pub fn register_identity_change_observer<F>(&self, callback: F)
    where
        F: Fn(Option<String>) + Send + Sync + 'static,
    {
        if let Ok(mut observers) = self.identity_change_observers.lock() {
            observers.push(Arc::new(callback));
        }
    }

    /// V-83 — clone of the kernel's `EventStore` publish-back slot (`Arc`).
    ///
    /// Returns the SAME `Arc<Mutex<Option<Arc<dyn EventStore>>>>` the actor
    /// publishes the kernel's store handle into after kernel construction (and
    /// re-publishes on `Reset`). Host code that needs a `'static` synchronous
    /// event reader (e.g. the V-80 OP-feed composition root's `event_lookup`
    /// closure, which outlives the `&NmpApp` borrow) captures this handle and
    /// reads through [`event_by_id_from_store`] on every call — so a `Reset`
    /// (which re-publishes a fresh store into the same slot) is observed without
    /// re-capturing. `None` inside the slot until the actor builds the kernel.
    ///
    /// Substrate-generic — the slot holds an `EventStore` handle; an event id
    /// maps to a `KernelEvent` with no NIP noun (D0). Parallel in shape to
    /// [`Self::active_account_handle`].
    #[must_use]
    pub fn event_store_handle(&self) -> EventStoreSlot {
        Arc::clone(&self.event_store_handle)
    }

    /// ADR-0058 step 3b — clone of the kernel's pull-cursor registry handle slot.
    ///
    /// Returns the SAME `Arc<Mutex<Option<PullCursorRegistrySlot>>>` the actor
    /// publishes the kernel's registry into after construction (and re-publishes
    /// on `Reset`). The synchronous [`crate::pull::nmp_app_pull_page`] path reads
    /// through it. `None` inside the slot until the actor builds the kernel.
    #[must_use]
    pub fn pull_cursor_registry_handle(&self) -> PullCursorRegistryHandleSlot {
        Arc::clone(&self.pull_cursor_registry)
    }

    /// #1740 step 2 — clone the feed-controller registry slot.
    ///
    /// A feed-session compiler captures this `Arc` into a `Send` teardown
    /// closure so `close_feed` can `unregister` the session's controller
    /// without holding `&NmpApp`. The slot is the SAME registry
    /// [`Self::register_feed`] writes into (single source of truth — D4).
    #[must_use]
    pub fn feed_registry_handle(&self) -> nmp_feed::FeedRegistrySlot {
        Arc::clone(&self.feed_registry)
    }

    /// #1740 step 2 — clone the snapshot-projection registry slot.
    ///
    /// Captured into a feed-session teardown closure to remove the session's
    /// typed sidecar projection on `close_feed`. Same slot
    /// [`Self::register_typed_snapshot_projection`] writes into.
    #[must_use]
    pub fn snapshot_projections_handle(&self) -> SnapshotProjectionSlot {
        Arc::clone(&self.snapshot_projections)
    }

    /// #1740 step 2 — clone the kernel-event-observer registry slot.
    ///
    /// Captured into a feed-session teardown closure to revoke the session's
    /// ingest observer by id on `close_feed`. Same slot
    /// [`Self::register_event_observer`] writes into (D4).
    #[must_use]
    pub fn event_observers_handle(&self) -> KernelEventObserverSlot {
        Arc::clone(&self.event_observers)
    }

    /// #1740 step 2 — clone the actor command sender.
    ///
    /// Captured into a feed-session teardown closure so it can post a
    /// `MarkChangedSinceEmit` after removing the session's registrations, making
    /// the next snapshot tick reflect the teardown. Cheap `Clone` over the same
    /// inbox the actor blocks on.
    #[must_use]
    pub fn command_sender(&self) -> nmp_core::CommandSender {
        self.tx.clone()
    }

    /// V-83 — synchronous event-by-id read against the kernel's event store.
    ///
    /// Reads the kernel-owned `EventStore` (published into the slot by the
    /// actor — the sole writer per D4) and returns the
    /// [`nmp_core::substrate::KernelEvent`] for `id` (a 64-char lowercase hex
    /// event id), or `None` when the store has
    /// no such event, `id` is malformed, the store has not been published yet
    /// (pre-`nmp_app_start`), or a lock is poisoned (D6 — degrades gracefully,
    /// never panics across the FFI boundary).
    ///
    /// `EventStore::get_by_id` is a `&self` read on a `Send + Sync` store; the
    /// store insert is ordered before the kernel-event observer fan-out
    /// (`kernel/ingest/timeline.rs`), so a synchronous read from a
    /// `KernelEventObserver` callback (which runs on the actor thread) observes
    /// the just-ingested event without re-entrancy: `insert` has already
    /// released the store's internal lock by the time the fan-out fires.
    #[must_use]
    pub fn event_by_id(&self, id: &str) -> Option<nmp_core::substrate::KernelEvent> {
        event_by_id_from_store(&self.event_store_handle, id)
    }

    /// ADR-0058 §8 step-6B — the in-process pull seam a feed's
    /// [`nmp_feed::PullFeedController`] drains on `load_older`.
    ///
    /// Returns a plain Rust closure `(scope, after_seq) -> page` that reads the
    /// kernel's published [`EventStore`](nmp_store::EventStore) directly
    /// via [`nmp_core::pull_page_over`]. This is **not** a new C-ABI symbol and
    /// **not** a projection accessor: it reads the raw ingest log exactly as the
    /// existing [`crate::pull::nmp_app_pull_page`] door does (ADR-0039 §6.1
    /// preserved — no host projection-pull accessor is added). The composition
    /// root hands this to `PullFeedController`; the host never sees it.
    ///
    /// Lock discipline mirrors `nmp_app_pull_page`: clone the store `Arc` under
    /// the slot lock, release it, then run the read against the clone. On an
    /// unavailable store (pre-`nmp_app_start` / poisoned) or an unsupported /
    /// erroring scope the closure returns an **empty, exhausted page** so the
    /// pager drain terminates and the feed fails closed (no broad-scan, no poll).
    #[must_use]
    pub fn feed_pull_fn(&self) -> nmp_feed::PullFn {
        use nmp_core::{pull_page_over, PullLimits};
        use nmp_store::{PullPage, ScanLogResult};
        use std::num::NonZeroUsize;

        let slot = Arc::clone(&self.event_store_handle);
        // One match entry per visible row; a generous per-call scan window. The
        // pager's own cross-call scan budget bounds total work (D5).
        let max_entries =
            NonZeroUsize::new(nmp_feed::DEFAULT_PULL_PAGE_SIZE).unwrap_or(NonZeroUsize::MIN);
        let max_scan = NonZeroUsize::new(nmp_feed::DEFAULT_PULL_PAGE_SIZE.saturating_mul(8))
            .unwrap_or(NonZeroUsize::MIN);
        let limits = PullLimits {
            max_entries,
            max_scan_entries: max_scan,
        };

        std::sync::Arc::new(move |scope: nmp_core::PullScope, after_seq: u64| {
            // Fail-closed terminator: an empty page at the requested cursor with
            // `has_more == false` ⇒ the drain stops as `Exhausted`, applies and
            // grows nothing, and `load_older` returns false (projection-only).
            let exhausted = || {
                ScanLogResult::Page(PullPage {
                    entries: Vec::new(),
                    next_after_seq: after_seq,
                    latest_seq: after_seq,
                    has_more: false,
                })
            };
            let store = {
                let Ok(guard) = slot.lock() else {
                    return exhausted();
                };
                match guard.as_ref() {
                    Some(s) => Arc::clone(s),
                    None => return exhausted(),
                }
            }; // slot lock released before the store read
            match pull_page_over(store.as_ref(), scope, after_seq, limits) {
                Ok(result) => result,
                // Unsupported shape / store error ⇒ fail closed, never broad-scan.
                Err(_) => exhausted(),
            }
        })
    }

    /// V-51 phase 4 — clone of the kernel's [`RoutingTraceProjection`]
    /// (`Arc`).
    ///
    /// Returns `None` until `nmp_app_start` spawns the actor and the actor has
    /// constructed the kernel. The projection is published into the slot
    /// immediately after construction — see `run_actor_with_observers`. Once
    /// `Some`, the same projection survives until `Reset`, which rebuilds the
    /// kernel and re-publishes a fresh projection clone into the slot.
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
        let raw = nmp_store::RawEvent {
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
}

impl nmp_core::substrate::ActionRegistrar for NmpApp {
    fn register_action<M: nmp_core::substrate::ActionModule + 'static>(&mut self, module: M) {
        NmpApp::register_action(self, module);
    }

    /// ADR-0049 Part 1 — override the trait default so the canonical NMP
    /// defaults (`nmp_nip02` / `nmp_nip17` / `nmp_nip57` / `nmp_router`, which
    /// register through `&mut impl AppHost`) get true entry-or-insert yielding
    /// semantics. Without this override the trait's default impl would delegate
    /// to `register_action` (the app path), recording every default as an app
    /// registration and making a repeated `register_defaults` collide.
    fn register_default_action<M: nmp_core::substrate::ActionModule + 'static>(
        &mut self,
        module: M,
    ) -> bool {
        NmpApp::register_default_action(self, module)
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

/// Register (or clear) the update callback on `app`.
///
/// # Quiescence contract
///
/// After this function returns, the previous `(callback, context)` pair is
/// guaranteed to be **neither registered nor mid-invocation**. Hosts may
/// safely free or release `context` once this call returns — no further
/// invocations of the old callback will occur, and any in-flight invocation
/// has completed before this function returns.
///
/// Pass `callback = None` (or `None` for the function pointer) to clear the
/// registration entirely. Passing a new `(context, callback)` pair replaces
/// the old one atomically from the perspective of the quiescence guarantee:
/// when this returns, the old registration is drained and the new one is
/// installed.
///
/// # Re-entrancy
///
/// A host callback **must not** call `nmp_app_set_update_callback` from
/// within the callback itself. The setter waits for in-flight invocations to
/// drain (via a `Condvar`), which cannot happen while the callback is
/// running on the listener thread — this would deadlock. No existing host
/// (iOS `nmpUpdateCallback`, Android `on_update`) calls back into the setter,
/// so this is not a live concern; it is documented so future implementors
/// know the invariant.
#[no_mangle]
pub extern "C" fn nmp_app_set_update_callback(
    app: *mut NmpApp,
    context: *mut c_void,
    callback: Option<UpdateCallback>,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let callback_registered = callback.is_some();
    let new_registration = callback.map(|callback| UpdateCallbackRegistration {
        context: context as usize,
        callback,
    });
    // Install the new registration (or clear) and then wait until any
    // in-flight invocation of the OLD registration has finished.
    //
    // The `Condvar::wait_while` loop re-acquires the mutex on each wakeup
    // and re-checks `in_flight == 0`, guarding against spurious wakeups.
    // The listener increments `in_flight` while holding this same mutex
    // before invoking the callback, so the wait condition is safe: if we
    // observe `in_flight == 0` under the lock, the listener has either not
    // yet incremented (and will see our cleared registration, so will not
    // increment) or has already decremented (and the callback has returned).
    let Ok(guard) = app.update_callback.inner.lock() else {
        return;
    };
    let mut guard = guard;
    guard.registration = new_registration;
    let waited = app
        .update_callback
        .drained
        .wait_while(guard, |inner| inner.in_flight > 0);
    // When `wait_while` returns, `in_flight == 0` under the lock. The old
    // registration has been replaced and no invocation of it is in flight.
    // Dropping `waited` releases the lock.
    drop(waited);
    if callback_registered && !app.started.load(Ordering::SeqCst) {
        app.emit_passive_prestart_snapshot();
    }
}

#[no_mangle]
pub extern "C" fn nmp_app_start(
    app: *mut NmpApp,
    visible_limit: c_uint,
    emit_hz: c_uint,
) {
    let Some(app) = app_ref(app) else {
        return;
    };

    // Read the pre-start initial relay configuration set by
    // `NmpAppBuilder::start()` (Rust path) or `set_initial_relays_for_start`.
    // Carried into `ActorCommand::Start` so the actor seeds `configured_relays`
    // before the session restore runs.
    let initial_relays = app
        .initial_relays_for_start
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default();

    // ADR-0053 / Workstream-E4 — LOUD forgotten-declaration check. Intent is
    // mandatory: an app must call `declare_consumed_projections` (narrow) or
    // `consume_all_builtin_projections` (explicit full set) before start. An
    // *undeclared* app is the forgotten-wiring footgun, NOT a silent
    // emit-everything opinion. The `debug_assert!` panics dev/prod debug builds;
    // release stays behaviour-preserving (`Undeclared` still permits every
    // built-in — never crashes, never goes dark) and surfaces a `tracing::warn!`.
    // The assert is compiled out under test harnesses (`test` / `test-support`):
    // those legitimately start undeclared and get the release behaviour. An
    // explicit `consume_all` (`All`) is NOT undeclared and never warns.
    if app.consumed_projections_are_undeclared() {
        tracing::warn!(
            "nmp_app_start: host expressed no projection-consumption intent — the \
             kernel will serialize all {} Tier-2 built-in projections on every tick \
             (including relay_diagnostics). This is a FORGOTTEN declaration, not an \
             opt-in: call `nmp_app_declare_consumed_projections` (narrow) or \
             `nmp_app_consume_all_builtin_projections` (explicit full set) before \
             start (ADR-0053 / Workstream-E4).",
            nmp_core::KERNEL_BUILTIN_PROJECTION_KEYS.len(),
        );
        #[cfg(not(any(test, feature = "test-support")))]
        debug_assert!(
            false,
            "nmp_app_start: projection-consumption intent is undeclared — call \
             nmp_app_declare_consumed_projections (narrow) or \
             nmp_app_consume_all_builtin_projections (explicit everything) before \
             start. No silent emit-everything default (ADR-0053 / Workstream-E4)."
        );
    }

    // ADR-0049 Part 2 — mark the app started BEFORE sending Start. From this
    // point the actor reads every wiring slot once at kernel construction, so a
    // later setter call is dropped and recorded as `DroppedLateWiring`. Set
    // before the send so there is no window where a setter racing in just after
    // Start records `ReplacedPrevious` instead of the truthful drop.
    let was_started = app.started.swap(true, Ordering::SeqCst);
    if !was_started {
        app.spawn_actor_if_needed();
    }

    app.send_cmd(ActorCommand::Start {
        visible_limit: clamp_visible(visible_limit),
        emit_hz: clamp_emit_hz(emit_hz),
        initial_relays,
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_configure(
    app: *mut NmpApp,
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
