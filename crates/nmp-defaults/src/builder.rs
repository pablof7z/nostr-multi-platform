//! `NmpAppBuilder` — typestate-guarded composition root for NMP-based apps.
//!
//! # V-94 — compile-time enforcement of pre-start ordering
//!
//! The problem: every wiring setter (`set_routing_substrate`,
//! `register_action`, …) must run **before** `nmp_app_start` — the actor reads
//! all wiring slots once at kernel construction; a later setter is silently
//! ignored (D6). The **consume-and-return typestate** enforces this in Rust:
//! `start(self, config)` moves the builder, so no setter is reachable
//! post-start. The C-ABI boundary (`nmp_app_*`) is outside Rust's type system;
//! a runtime late-wiring diagnostic is the complement there (not in this PR).
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
//! use nmp_defaults::{NmpAppBuilder, RunConfig};
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
//! This type lives in `nmp-defaults` and targets **Rust composition
//! roots** (`nmp_app_chirp_register`, fixture helpers, future second apps).
//! It does NOT modify the C-ABI surface (`nmp_app_*` symbols) or any
//! Swift/Kotlin code — those remain unchanged.
//!
//! [`AppHost`]: nmp_core::substrate::AppHost

use std::marker::PhantomData;
use std::sync::Arc;

use nmp_core::substrate::{ActionRegistrar, AppHost};
use nmp_ffi::{nmp_app_free, nmp_app_new, nmp_app_start, NmpApp};

use crate::relay_config;
mod app_host_impl; // ADR-0053: `impl AppHost for NmpAppBuilder` child submodule (LOC ceiling).
mod wallet; // `with_wallet` (NIP-47 wiring) — child submodule; see builder/wallet.rs.

/// The app-template's built-in default relay configuration.
///
/// Used when the caller declares no relays via `with_relay`/`with_relays` and
/// no persisted sidecar exists. This is the composition-root home for the
/// default relay set — `nmp-core` no longer carries any hardcoded fallback.
const DEFAULT_APP_RELAYS: &[(&str, &str)] = &[
    ("wss://relay.primal.net", "both,indexer"),
    ("wss://purplepag.es", "indexer"),
];

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
/// use nmp_defaults::{NmpAppBuilder, RunConfig};
///
/// // ERROR: no method named `start` found for `NmpAppBuilder<Unstarted>`
/// let _app = NmpAppBuilder::new().start(RunConfig::default());
/// ```
///
/// The correct sequence is:
///
/// ```rust,no_run
/// use nmp_defaults::{NmpAppBuilder, RunConfig};
///
/// let _app = NmpAppBuilder::new()
///     .in_memory()                  // ← required: advance to StorageSet
///     .start(RunConfig::default()); // ← now compiles
/// ```
pub struct NmpAppBuilder<S> {
    /// Owned pointer. INVARIANT: non-null while the builder exists; freed
    /// either by `start()` (released to the runtime) or by `Drop`.
    app: *mut NmpApp,
    /// App-declared relay overrides. `None` ⇒ use [`DEFAULT_APP_RELAYS`];
    /// `Some(v)` ⇒ the caller declared these via `with_relay`/`with_relays`.
    /// Resolved into the kernel's `configured_relays` (via the JSON sidecar)
    /// at `start()`.
    user_relays: Option<Vec<(String, String)>>,
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
            user_relays: None,
            _state: PhantomData,
        }
    }
}

// ── Relay declaration (both states) ─────────────────────────────────────────

impl<S> NmpAppBuilder<S> {
    /// Declare a relay the app wants to start with. Multiple calls accumulate.
    ///
    /// The first call clears the built-in [`DEFAULT_APP_RELAYS`] (so declaring
    /// any relay replaces the defaults entirely); subsequent calls append.
    ///
    /// `role` is a relay-role string (`"read"`, `"write"`, `"both"`,
    /// `"indexer"`, or a composite like `"both,indexer"`). It is canonicalized
    /// by the kernel when the row is seeded.
    #[must_use]
    pub fn with_relay(mut self, url: impl Into<String>, role: impl Into<String>) -> Self {
        self.user_relays
            .get_or_insert_with(Vec::new)
            .push((url.into(), role.into()));
        self
    }

    /// Declare several relays at once. Equivalent to calling [`Self::with_relay`]
    /// for each `(url, role)` pair. The first declaration (here or via
    /// `with_relay`) clears the built-in defaults.
    #[must_use]
    pub fn with_relays(
        mut self,
        relays: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        let list = self.user_relays.get_or_insert_with(Vec::new);
        for (url, role) in relays {
            list.push((url.into(), role.into()));
        }
        self
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
        // Transfer ownership to the new builder WITHOUT running our own Drop.
        // `*mut NmpApp` is `Copy`, but `user_relays` (a `Vec`) is not — a plain
        // field move would hit E0509 ("cannot move out of type which implements
        // Drop"). `ptr::read` byte-copies the non-Copy field out, then
        // `mem::forget(self)` suppresses the destructor so neither the
        // `NmpApp` is freed nor `user_relays` is double-dropped. Ownership of
        // both transfers to the returned builder.
        let app = self.app;
        let user_relays = unsafe { std::ptr::read(&self.user_relays) };
        std::mem::forget(self);
        NmpAppBuilder {
            app,
            user_relays,
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
        // Transfer ownership WITHOUT running Drop (same pattern as
        // `storage_path` and `start` — see `storage_path` for the rationale,
        // including why `user_relays` needs `ptr::read`).
        let app = self.app;
        let user_relays = unsafe { std::ptr::read(&self.user_relays) };
        std::mem::forget(self);
        NmpAppBuilder {
            app,
            user_relays,
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
        // Move the non-Copy `user_relays` out before forgetting `self` (same
        // E0509 rationale as the storage transitions).
        let user_relays = unsafe { std::ptr::read(&self.user_relays) };
        // Prevent `Drop` from double-freeing: consume `self` without running
        // the drop glue. The caller takes ownership of `app`.
        std::mem::forget(self);

        // Resolve the app's declared default relay set: caller-declared relays
        // if any, else the built-in `DEFAULT_APP_RELAYS`.
        let relay_defaults: Vec<(String, String)> = user_relays.unwrap_or_else(|| {
            DEFAULT_APP_RELAYS
                .iter()
                .map(|(u, r)| (u.to_string(), r.to_string()))
                .collect()
        });

        // Decide the initial relay set:
        //   * Persistent store → load the JSON sidecar from the storage dir; on
        //     first run (no sidecar) persist the declared defaults, then use
        //     them. Subsequent runs reload the user's edited list.
        //   * In-memory store → no disk I/O; use the declared defaults directly.
        //
        // SAFETY: `app` is non-null (builder invariant) and not yet started, so
        // a shared borrow to read the storage path is sound.
        let initial_relays: Vec<(String, String)> = match unsafe { &*app }.storage_path_for_start()
        {
            Some(path) if !path.trim().is_empty() => {
                let dir = std::path::Path::new(&path);
                match relay_config::load(dir) {
                    Some(loaded) => loaded,
                    None => {
                        relay_config::save(dir, &relay_defaults);
                        relay_defaults
                    }
                }
            }
            // No storage path (in-memory) — use defaults, no sidecar.
            _ => relay_defaults,
        };

        // Stage the initial relays BEFORE start so `nmp_app_start` carries them
        // in `ActorCommand::Start { initial_relays }`. MUST precede the start.
        // SAFETY: `app` non-null; not yet started.
        unsafe { &*app }.set_initial_relays_for_start(initial_relays);

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
    fn register_action<M: nmp_core::substrate::ActionModule + 'static>(&mut self, module: M) {
        // SAFETY: `self.app` non-null (builder invariant). Exclusive borrow via
        // `&mut self` ⇒ no aliasing.
        let app: &mut NmpApp = unsafe { &mut *self.app };
        app.register_action(module);
    }

    /// Route the yielding-default path to `NmpApp::register_default_action` (the
    /// kernel's true entry-or-insert semantics), NOT the trait default — which
    /// delegates to the *app* path and would record every canonical NMP default
    /// as an app registration, so a later app-path override of the same
    /// namespace (e.g. ADR-0052 rung 5.2's `register_zap_with_wallet`) trips the
    /// app-over-app collision `debug_assert!` instead of cleanly replacing a
    /// `Provenance::Default` entry (ADR-0049 Part 1).
    fn register_default_action<M: nmp_core::substrate::ActionModule + 'static>(
        &mut self,
        module: M,
    ) -> bool {
        // SAFETY: as `register_action` above.
        let app: &mut NmpApp = unsafe { &mut *self.app };
        app.register_default_action(module)
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
