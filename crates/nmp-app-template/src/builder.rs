//! `NmpAppBuilder` — typestate-guarded composition root for NMP-based apps.
//!
//! # V-94 — compile-time enforcement of pre-start ordering
//!
//! The problem: `nmp_app_new()` allocates an un-started `NmpApp`; every
//! wiring setter (`set_routing_substrate`, `register_action`, etc.) must be
//! called **before** `nmp_app_start` sends the first `ActorCommand::Start`.
//! The actor reads all wiring slots once, at kernel-construction time; any
//! setter called *after* that point is silently ignored (D6). Up to this PR
//! ordering was enforced by prose only (18 "MUST be called before
//! `nmp_app_start`" doc-block sites in `nmp-ffi/src/lib.rs`).
//!
//! # Design decisions (V-94 ABI fork — resolved in task brief)
//!
//! The task explicitly chose the **consume-and-return typestate** approach:
//! `start(self, config)` moves the builder, so no setter is reachable
//! post-start in Rust. This is stronger than an in-place `started` flag
//! (which would still compile at the wrong call site) and stronger than a
//! runtime check (which fires at runtime, not compile time).
//!
//! The **C-ABI boundary** (`nmp_app_start`, `nmp_app_set_*`) is outside the
//! reach of Rust's type system. Swift/Kotlin hosts driving raw C-ABI symbols
//! get no compile-time guarantee here. A runtime late-wiring diagnostic
//! (`KernelDiagnostic::LateWiring`) is the correct complement for that surface
//! — it is **not** implemented in this PR (scope: Rust composition roots only).
//!
//! # Type-state chain
//!
//! ```text
//! NmpAppBuilder<Unstarted>
//!       │  .storage_path(p)   ─┐
//!       │  .in_memory()       ─┤─→  NmpAppBuilder<StorageSet>
//!       │                       │         │
//!       │ (AppHost + ActionRegistrar       │  .start(RunConfig)
//!       │  setters available on BOTH       │        │
//!       │  states — they don't advance     ▼        ▼
//!       │  the required chain)         StartedApp (*mut NmpApp, running)
//!       │
//!       ╰─ .start(RunConfig) — DOES NOT COMPILE (only on StorageSet)
//! ```
//!
//! # Usage (canonical Rust composition root)
//!
//! ```rust,no_run
//! use nmp_app_template::{NmpAppBuilder, RunConfig};
//!
//! let app: *mut nmp_ffi::NmpApp = NmpAppBuilder::new()
//!     .in_memory()                  // required: choose storage
//!     .start(RunConfig::default()); // consume builder → started handle
//!
//! // `NmpAppBuilder` is gone; setters are unreachable.
//! // Use `app` for FFI calls; free with `nmp_ffi::nmp_app_free(app)`.
//! ```
//!
//! The canonical production step replaces `.in_memory()` with
//! `.storage_path("/path/to/lmdb/dir")`.
//!
//! # Scope
//!
//! This type lives in `nmp-app-template` and targets **Rust composition
//! roots** (`nmp_app_chirp_register`, fixture helpers, future second apps).
//! It does NOT modify the C-ABI surface (`nmp_app_*` symbols) or any
//! Swift/Kotlin code — those remain unchanged.
//!
//! [`AppHost`]: nmp_core::substrate::AppHost

use std::marker::PhantomData;
use std::sync::Arc;

use nmp_core::substrate::{ActionRegistrar, AppHost};
use nmp_ffi::{nmp_app_free, nmp_app_new, nmp_app_start, NmpApp};

// ── Type-state markers ───────────────────────────────────────────────────────

/// Builder state: no storage decision made yet.
///
/// `start()` is NOT available in this state — call `.storage_path(p)` or
/// `.in_memory()` first.
pub struct Unstarted;

/// Builder state: storage has been explicitly chosen.
///
/// Either `.storage_path(p)` (LMDB-backed) or `.in_memory()` (explicit
/// ephemeral opt-in) was called. `start()` is now available.
pub struct StorageSet;

// ── RunConfig ────────────────────────────────────────────────────────────────

/// Runtime configuration forwarded to `nmp_app_start`.
///
/// Mirrors the three parameters `nmp_app_start` accepts today:
/// `visible_limit` (max rows the kernel emits per snapshot) and `emit_hz`
/// (snapshot-emission rate). A third parameter (`_events_per_second`) is
/// accepted by the C-ABI but ignored; it is omitted here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunConfig {
    /// Maximum number of feed rows the kernel includes in each snapshot.
    /// Forwarded to `nmp_app_start` as `visible_limit`. Clamped to [1, 1000]
    /// by the C-ABI.
    pub visible_limit: u32,
    /// Snapshot-emission rate in Hz. Forwarded to `nmp_app_start` as
    /// `emit_hz`. Clamped to [1, 60] by the C-ABI.
    pub emit_hz: u32,
}

impl Default for RunConfig {
    /// Sensible production defaults: 100 visible rows, 4 Hz snapshot rate.
    ///
    /// These match the defaults the iOS Chirp host passes in practice.
    fn default() -> Self {
        Self {
            visible_limit: 100,
            emit_hz: 4,
        }
    }
}

// ── NmpAppBuilder ────────────────────────────────────────────────────────────

/// Typestate builder for an NMP-based application.
///
/// Owns the `*mut NmpApp` from `nmp_app_new()` during the wiring phase and
/// guarantees at compile time that:
///
/// 1. All `AppHost`/`ActionRegistrar` setters (action modules, routing
///    substrate, coverage hook, …) run **before** `start()`.
/// 2. A storage decision (`.storage_path` or `.in_memory()`) is made before
///    `start()` — the one slot whose omission causes silent data loss.
/// 3. `start()` is callable **exactly once** (it moves `self`).
///
/// On `Drop`, if `start()` was never called, the inner `NmpApp` is freed with
/// `nmp_app_free` to prevent a memory leak.
///
/// # Type parameter
///
/// `S` is a zero-size type-state marker. Use `NmpAppBuilder<Unstarted>` as
/// the initial type; advance to `NmpAppBuilder<StorageSet>` via
/// `.storage_path(p)` or `.in_memory()`.
///
/// # Compile-fail: calling `start()` without a storage choice is an error
///
/// The following code does **not** compile because `start()` only exists on
/// `NmpAppBuilder<StorageSet>`, not on `NmpAppBuilder<Unstarted>`:
///
/// ```compile_fail
/// use nmp_app_template::{NmpAppBuilder, RunConfig};
///
/// // ERROR: no method named `start` found for `NmpAppBuilder<Unstarted>`
/// let _app = NmpAppBuilder::new().start(RunConfig::default());
/// ```
///
/// The correct sequence is:
///
/// ```rust,no_run
/// use nmp_app_template::{NmpAppBuilder, RunConfig};
///
/// let _app = NmpAppBuilder::new()
///     .in_memory()                  // ← required: advance to StorageSet
///     .start(RunConfig::default()); // ← now compiles
/// ```
pub struct NmpAppBuilder<S> {
    /// Owned pointer. INVARIANT: non-null while the builder exists; freed
    /// either by `start()` (released to the runtime) or by `Drop`.
    app: *mut NmpApp,
    _state: PhantomData<S>,
}

// SAFETY: `NmpApp` is built to be sent across threads (it is `Send` on the
// nmp-ffi side). The builder's raw pointer is owned exclusively by the
// builder instance; no alias exists until `start()` returns it.
// `PhantomData<S>` is always `Send + Sync` for our ZST markers.
unsafe impl<S> Send for NmpAppBuilder<S> {}
unsafe impl<S> Sync for NmpAppBuilder<S> {}

impl NmpAppBuilder<Unstarted> {
    /// Allocate a fresh `NmpApp` and enter the wiring phase.
    ///
    /// # Panics
    ///
    /// Panics when `nmp_app_new()` returns null (out-of-memory or internal
    /// initialisation failure — in practice this never occurs on a healthy
    /// process).
    pub fn new() -> Self {
        let app = nmp_app_new();
        assert!(!app.is_null(), "nmp_app_new() returned null");
        Self {
            app,
            _state: PhantomData,
        }
    }
}

impl Default for NmpAppBuilder<Unstarted> {
    fn default() -> Self {
        Self::new()
    }
}

// ── Storage-selection transitions (Unstarted → StorageSet) ──────────────────

impl NmpAppBuilder<Unstarted> {
    /// Use a persistent LMDB store at `path`.
    ///
    /// Transitions to `NmpAppBuilder<StorageSet>`, enabling `start()`.
    ///
    /// In practice `path` is the host-provided application-support directory
    /// (iOS) or files directory (Android). A `NULL` or empty `path` passed to
    /// the underlying C-ABI falls back to the `NMP_LMDB_PATH` env var, then
    /// the in-memory store.
    ///
    /// # Panics
    ///
    /// Does not panic; an empty or invalid path is silently treated as "unset"
    /// by the C-ABI setter (same behaviour as a direct call to
    /// `nmp_app_set_storage_path`).
    pub fn storage_path(self, path: impl Into<String>) -> NmpAppBuilder<StorageSet> {
        let path_string = path.into();
        // There is no dedicated Rust-internal setter on `NmpApp` for the
        // storage path today (the only write path is the C-ABI
        // `nmp_app_set_storage_path`). We convert the Rust `String` to a
        // nul-terminated `CString` and call through to the C-ABI setter —
        // the same code path the host (iOS/Android) takes.
        set_storage_path_via_cabi(self.app, &path_string);
        // Transfer pointer ownership to the new builder WITHOUT running our
        // own Drop: `*mut NmpApp` is `Copy`, so writing `self.app` would
        // copy the pointer and then Drop on `self` would double-free it.
        // `mem::forget` suppresses the builder's destructor so `NmpApp::drop`
        // does not run here (the pointer was copied out above; ownership
        // transfers to the returned builder).
        let app = self.app;
        std::mem::forget(self);
        NmpAppBuilder {
            app,
            _state: PhantomData,
        }
    }

    /// Use an ephemeral in-memory store (explicit opt-in).
    ///
    /// This transitions to `NmpAppBuilder<StorageSet>` and enables `start()`.
    /// An in-memory store loses all events when the process exits — this opt-in
    /// makes that choice explicit and visible in code, unlike the old silent
    /// default where omitting `nmp_app_set_storage_path` gave in-memory
    /// storage without any declaration.
    ///
    /// Suitable for tests and short-lived tools. For production apps use
    /// `.storage_path(p)` instead.
    pub fn in_memory(self) -> NmpAppBuilder<StorageSet> {
        // Leave the storage-path slot at `None` (its default from
        // `nmp_app_new`). The actor thread then falls back to the in-memory
        // `EventStore` — same behaviour as before, but now the caller has
        // explicitly opted in.
        //
        // Transfer pointer ownership WITHOUT running Drop (same pattern as
        // `storage_path` and `start` — see `storage_path` for the rationale).
        let app = self.app;
        std::mem::forget(self);
        NmpAppBuilder {
            app,
            _state: PhantomData,
        }
    }
}

// ── Terminal transition: start (StorageSet only) ─────────────────────────────

impl NmpAppBuilder<StorageSet> {
    /// Consume the builder and start the NMP kernel.
    ///
    /// This is the **only** path from `NmpAppBuilder<StorageSet>` to a live
    /// `*mut NmpApp`. It:
    ///
    /// 1. Calls `nmp_app_start` with the given `RunConfig`.
    /// 2. Releases ownership of the `NmpApp` pointer to the caller.
    ///
    /// After this call, the builder is gone — no setter is reachable (compile
    /// error). The returned pointer is owned by the caller; free it with
    /// `nmp_ffi::nmp_app_free`.
    ///
    /// # Safety
    ///
    /// The returned pointer is a valid, non-null `*mut NmpApp`. The caller is
    /// responsible for eventual `nmp_app_free(ptr)`.
    pub fn start(self, config: RunConfig) -> *mut NmpApp {
        let app = self.app;
        // Prevent `Drop` from double-freeing: consume `self` without running
        // the drop glue. The caller takes ownership of `app`.
        std::mem::forget(self);
        // SAFETY: `app` is non-null (builder invariant).
        nmp_app_start(app, 0, config.visible_limit, config.emit_hz);
        app
    }
}

// ── AppHost + ActionRegistrar delegations (both states) ─────────────────────
//
// Every wiring method is available in BOTH `Unstarted` and `StorageSet`.
// They don't advance the required chain — the only constraint is that they
// run before `start()`, which the typestate already guarantees.

impl<S> ActionRegistrar for NmpAppBuilder<S> {
    fn register_action<M: nmp_core::substrate::ActionModule + 'static>(&mut self) {
        // SAFETY: `self.app` non-null (builder invariant). Exclusive borrow via
        // `&mut self` ⇒ no aliasing.
        let app: &mut NmpApp = unsafe { &mut *self.app };
        app.register_action::<M>();
    }
}

impl<S> AppHost for NmpAppBuilder<S> {
    fn register_snapshot_projection<K, F>(&self, key: K, f: F)
    where
        K: Into<String>,
        F: Fn() -> serde_json::Value + Send + Sync + 'static,
    {
        // SAFETY: `self.app` non-null (builder invariant). Shared borrow via
        // `&self` is safe — all AppHost methods take `&self`.
        let app: &NmpApp = unsafe { &*self.app };
        app.register_snapshot_projection(key, f);
    }

    fn set_coverage_hook(&self, hook: nmp_core::subs::PlanCoverageHook) {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_coverage_hook(hook);
    }

    fn set_req_frame_interceptor(
        &self,
        interceptor: Arc<dyn nmp_core::substrate::ReqFrameInterceptor>,
    ) {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_req_frame_interceptor(interceptor);
    }

    fn add_relay_text_interceptor(
        &self,
        interceptor: Arc<dyn nmp_core::substrate::RelayTextInterceptor>,
    ) {
        let app: &NmpApp = unsafe { &*self.app };
        app.add_relay_text_interceptor(interceptor);
    }

    fn register_ingest_parser(
        &self,
        kind: u32,
        parser: Arc<dyn nmp_core::substrate::IngestParser>,
    ) {
        let app: &NmpApp = unsafe { &*self.app };
        app.register_ingest_parser(kind, parser);
    }

    fn set_dm_inbox_relay_lookup(
        &self,
        lookup: Arc<dyn nmp_core::substrate::DmInboxRelayLookup>,
    ) {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_dm_inbox_relay_lookup(lookup);
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
        let app: &NmpApp = unsafe { &*self.app };
        app.set_routing_substrate(factory);
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
        let app: &NmpApp = unsafe { &*self.app };
        app.set_publish_resolver_factory(factory);
    }

    fn set_raw_event_forward_policy_factory<F>(&self, factory: F)
    where
        F: Fn(
                nmp_core::substrate::RawEventForwardPolicyContext,
            ) -> Vec<Arc<dyn nmp_core::substrate::RawEventForwardPolicy>>
            + Send
            + Sync
            + 'static,
    {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_raw_event_forward_policy_factory(factory);
    }

    fn active_local_keys(&self) -> nmp_core::slots::ActiveLocalKeysSlot {
        let app: &NmpApp = unsafe { &*self.app };
        app.active_local_keys()
    }

    fn actor_sender(&self) -> std::sync::mpsc::Sender<nmp_core::ActorCommand> {
        let app: &NmpApp = unsafe { &*self.app };
        app.actor_sender()
    }

    fn register_event_observer(
        &self,
        observer: Arc<dyn nmp_core::KernelEventObserver>,
    ) -> nmp_core::KernelEventObserverId {
        let app: &NmpApp = unsafe { &*self.app };
        app.register_event_observer(observer)
    }

    fn unregister_event_observer(&self, id: nmp_core::KernelEventObserverId) {
        let app: &NmpApp = unsafe { &*self.app };
        app.unregister_event_observer(id);
    }

    fn swap_singleton_event_observer(
        &self,
        new: Option<nmp_core::KernelEventObserverId>,
    ) -> Option<nmp_core::KernelEventObserverId> {
        let app: &NmpApp = unsafe { &*self.app };
        app.swap_singleton_event_observer(new)
    }

    fn register_raw_event_observer(
        &self,
        kinds: nmp_core::KindFilter,
        observer: Arc<dyn nmp_core::RawEventObserver>,
    ) -> nmp_core::RawEventObserverId {
        let app: &NmpApp = unsafe { &*self.app };
        app.register_raw_event_observer(kinds, observer)
    }

    fn unregister_raw_event_observer(&self, id: nmp_core::RawEventObserverId) {
        let app: &NmpApp = unsafe { &*self.app };
        app.unregister_raw_event_observer(id);
    }

    fn swap_dm_inbox_observer(
        &self,
        new: Option<nmp_core::RawEventObserverId>,
    ) -> Option<nmp_core::RawEventObserverId> {
        let app: &NmpApp = unsafe { &*self.app };
        app.swap_dm_inbox_observer(new)
    }

    fn relay_edit_rows_handle(&self) -> nmp_core::RelayEditRowsSlot {
        let app: &NmpApp = unsafe { &*self.app };
        app.relay_edit_rows_handle()
    }

    fn set_nostrconnect_bootstrap_relay(&self, url: String) {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_nostrconnect_bootstrap_relay(url);
    }
}

// ── Drop guard ───────────────────────────────────────────────────────────────

impl<S> Drop for NmpAppBuilder<S> {
    /// Free the inner `NmpApp` if `start()` was never called.
    ///
    /// This prevents a memory leak when a builder is constructed but then
    /// dropped without starting (e.g. after an error during wiring).
    fn drop(&mut self) {
        // `start()` uses `mem::forget(self)` to bypass this destructor, so
        // this branch is only reached when the builder is dropped without
        // starting.
        if !self.app.is_null() {
            // SAFETY: `self.app` is non-null and owned exclusively by the
            // builder (invariant). `start()` used `mem::forget` so this is
            // the sole drop point.
            nmp_app_free(self.app);
        }
    }
}

// ── C-ABI storage-path helper ─────────────────────────────────────────────

/// Write the storage path into the `NmpApp`'s `storage_path` slot via the
/// C-ABI `nmp_app_set_storage_path` — the only public write path to that
/// field (no `pub` Rust method exists on `NmpApp` today).
///
/// Converts the Rust `&str` to a nul-terminated `CString`, then calls the
/// C-ABI function. This is the correct pattern for callers that hold a Rust
/// `*mut NmpApp` and want to set the storage path without re-inventing the
/// slot-locking logic.
fn set_storage_path_via_cabi(app: *mut NmpApp, path: &str) {
    use std::ffi::CString;
    // NUL bytes in the path are pathological; reject silently (the C-ABI
    // treats an empty/NULL path as "unset", so this degrades to in-memory
    // rather than panicking).
    let Ok(c_path) = CString::new(path) else {
        return;
    };
    // SAFETY contract (for the human reader): `app` is non-null (builder
    // invariant); `c_path` is a valid nul-terminated C string live for the
    // duration of the call. Rust does not require an `unsafe` block to call
    // `pub extern "C"` functions exported from another Rust crate — the
    // unsafety is the caller's responsibility by convention.
    nmp_ffi::nmp_app_set_storage_path(app, c_path.as_ptr());
}
