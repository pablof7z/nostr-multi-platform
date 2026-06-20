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
//!       │ (AppHost + ActionRegistrar       │  .declare_consumed_projections(keys)   ─┐
//!       │  setters available on ALL        │  .consume_all_builtin_projections()    ─┤
//!       │  states — they don't advance     │                                          │
//!       │  the required chain)             ▼                                          ▼
//!       │                          (no .start here)            NmpAppBuilder<ProjectionsDeclared>
//!       │                                                                  │  .with_relays(iter)        ─┐
//!       │                                                                  │  .without_initial_relays() ─┤
//!       │                                                                  ▼                             ▼
//!       │                                                          (no .start here)         NmpAppBuilder<RelaysDeclared>
//!       │                                                                                              │  .start(RunConfig)
//!       │                                                                                              ▼
//!       │                                                                              StartedApp (*mut NmpApp, running)
//!       │
//!       ╰─ .start(RunConfig) — DOES NOT COMPILE until RelaysDeclared
//! ```
//!
//! Three compile-time gates: (1) a storage decision (`Unstarted → StorageSet`),
//! (2) an ADR-0053 projection-consumption decision
//! (`StorageSet → ProjectionsDeclared`), and (3) a #1493 initial-relay decision
//! (`ProjectionsDeclared → RelaysDeclared`). Forgetting any is a compile error,
//! so a host can never silently ship the full Tier-2 built-in firehose
//! ("everything" is the explicit `.consume_all_builtin_projections()` call) nor
//! silently inherit a framework relay default (NMP has none — relays are
//! leaf-app policy supplied via `.with_relays(...)` or the explicit
//! `.without_initial_relays()`).
//!
//! # Usage (canonical Rust composition root)
//!
//! ```rust,no_run
//! use nmp_defaults::{NmpAppBuilder, RunConfig};
//!
//! let app: *mut nmp_ffi::NmpApp = NmpAppBuilder::new()
//!     .in_memory()                                  // required: choose storage
//!     .declare_consumed_projections(["profile"])    // required: declare ADR-0053 set
//!     .with_relays([("wss://your.relay", "both")])  // required: declare relays (#1493)
//!     .start(RunConfig::default());                 // consume builder → started handle
//!
//! // `NmpAppBuilder` is gone; setters are unreachable.
//! // Use `app` for FFI calls; free with `nmp_ffi::nmp_app_free(app)`.
//! ```
//!
//! The canonical production step replaces `.in_memory()` with
//! `.storage_path("/path/to/lmdb/dir")`. A diagnostics shell that genuinely
//! reads every built-in calls `.consume_all_builtin_projections()` in place of
//! `.declare_consumed_projections(...)`.
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

use nmp_core::substrate::ActionRegistrar;
use nmp_ffi::{nmp_app_free, nmp_app_new, nmp_app_start, NmpApp};

use crate::relay_config;
mod app_host_impl; // ADR-0053: `impl AppHost for NmpAppBuilder` child submodule (LOC ceiling).
mod wallet; // `with_wallet` (NIP-47 wiring) — child submodule; see builder/wallet.rs.

// ── Type-state markers ───────────────────────────────────────────────────────

/// Builder state: no storage decision made yet.
///
/// `start()` is NOT available in this state — call `.storage_path(p)` or
/// `.in_memory()` first.
pub struct Unstarted;

/// Builder state: storage has been explicitly chosen.
///
/// Either `.storage_path(p)` (LMDB-backed) or `.in_memory()` (explicit
/// ephemeral opt-in) was called. `start()` is NOT yet available — a
/// projection-consumption decision must be made first (ADR-0053 DEBT 2):
/// call `.declare_consumed_projections(keys)` to narrow to a set, or
/// `.consume_all_builtin_projections()` to explicitly opt into the full
/// Tier-2 firehose.
pub struct StorageSet;

/// Builder state: the host has made an explicit projection-consumption
/// decision (ADR-0053 DEBT 2).
///
/// Reached from `StorageSet` via either `.declare_consumed_projections(keys)`
/// (narrowing) or `.consume_all_builtin_projections()` (explicit "I want
/// everything"). `start()` is available ONLY in this state — so a host
/// CANNOT silently ship the full Tier-2 built-in firehose by forgetting to
/// decide; "everything" is a visible, greppable, intentional call, never a
/// default. The kernel-primitive empty=permissive semantic
/// (`SnapshotRegistry`/`NmpApp`) is unchanged; only the app-facing builder
/// gains this compile-time gate.
pub struct ProjectionsDeclared;

/// Builder state: the app has made an explicit initial-relay decision (#1493).
///
/// Reached from `ProjectionsDeclared` via either `.with_relays(...)` (declare
/// the relay set the app starts with) or `.without_initial_relays()` (explicit
/// "this app ships no built-in relays"). `start()` is available ONLY in this
/// state — so an app CANNOT silently inherit a framework relay default by
/// forgetting to decide. NMP (including `nmp-defaults`) owns no operator relay
/// URLs; the relay set is leaf-app policy supplied here.
pub struct RelaysDeclared;

// ── RunConfig ────────────────────────────────────────────────────────────────

/// Runtime configuration forwarded to `nmp_app_start`.
///
/// Mirrors the two parameters `nmp_app_start` accepts:
/// `visible_limit` (max rows the kernel emits per snapshot) and `emit_hz`
/// (snapshot-emission rate).
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
/// 3. An ADR-0053 projection-consumption decision
///    (`.declare_consumed_projections(keys)` or
///    `.consume_all_builtin_projections()`) is made before `start()` — so the
///    full Tier-2 built-in firehose can never be shipped silently.
/// 4. `start()` is callable **exactly once** (it moves `self`).
///
/// On `Drop`, if `start()` was never called, the inner `NmpApp` is freed with
/// `nmp_app_free` to prevent a memory leak.
///
/// # Type parameter
///
/// `S` is a zero-size type-state marker. Use `NmpAppBuilder<Unstarted>` as
/// the initial type; advance to `NmpAppBuilder<StorageSet>` via
/// `.storage_path(p)` or `.in_memory()`, then to
/// `NmpAppBuilder<ProjectionsDeclared>` via `.declare_consumed_projections(…)`
/// or `.consume_all_builtin_projections()`, then to
/// `NmpAppBuilder<RelaysDeclared>` via `.with_relays(…)` or
/// `.without_initial_relays()`.
///
/// # Compile-fail: calling `start()` without a storage choice is an error
///
/// The following code does **not** compile because `start()` only exists on
/// `NmpAppBuilder<ProjectionsDeclared>`, not on `NmpAppBuilder<Unstarted>`:
///
/// ```compile_fail
/// use nmp_defaults::{NmpAppBuilder, RunConfig};
///
/// // ERROR: no method named `start` found for `NmpAppBuilder<Unstarted>`
/// let _app = NmpAppBuilder::new().start(RunConfig::default());
/// ```
///
/// # Compile-fail: calling `start()` without a projection decision is an error
///
/// The following code does **not** compile because `start()` only exists on
/// `NmpAppBuilder<ProjectionsDeclared>`, not on `NmpAppBuilder<StorageSet>` —
/// ADR-0053 DEBT 2's compile-time enforcement that the host cannot silently
/// ship the full Tier-2 built-in firehose:
///
/// ```compile_fail
/// use nmp_defaults::{NmpAppBuilder, RunConfig};
///
/// // ERROR: no method named `start` found for `NmpAppBuilder<StorageSet>`
/// let _app = NmpAppBuilder::new()
///     .in_memory()                  // advances to StorageSet
///     .start(RunConfig::default()); // ← still missing the projection decision
/// ```
///
/// # Compile-fail: calling `start()` without a relay decision is an error
///
/// The following code does **not** compile because `start()` only exists on
/// `NmpAppBuilder<RelaysDeclared>`, not on `NmpAppBuilder<ProjectionsDeclared>`
/// — #1493's compile-time enforcement that the app cannot silently inherit a
/// framework relay default (NMP has none):
///
/// ```compile_fail
/// use nmp_defaults::{NmpAppBuilder, RunConfig};
///
/// // ERROR: no method named `start` found for `NmpAppBuilder<ProjectionsDeclared>`
/// let _app = NmpAppBuilder::new()
///     .in_memory()
///     .consume_all_builtin_projections() // advances to ProjectionsDeclared
///     .start(RunConfig::default());      // ← still missing the relay decision
/// ```
///
/// The correct sequence is:
///
/// ```rust,no_run
/// use nmp_defaults::{NmpAppBuilder, RunConfig};
///
/// let _app = NmpAppBuilder::new()
///     .in_memory()                               // ← required: advance to StorageSet
///     .declare_consumed_projections(["profile"]) // ← required: advance to ProjectionsDeclared
///     .without_initial_relays()                  // ← required: advance to RelaysDeclared
///     .start(RunConfig::default());              // ← now compiles
/// ```
pub struct NmpAppBuilder<S> {
    /// Owned pointer. INVARIANT: non-null while the builder exists; freed
    /// either by `start()` (released to the runtime) or by `Drop`.
    app: *mut NmpApp,
    /// App-declared initial relay set (#1493). Empty until the app makes the
    /// explicit `ProjectionsDeclared → RelaysDeclared` decision via
    /// `.with_relays(...)` (non-empty) or `.without_initial_relays()` (empty).
    /// Resolved into the kernel's `configured_relays` (via the JSON sidecar) at
    /// `start()`. NMP carries no relay fallback — what the app declares here is
    /// exactly what the kernel starts with.
    user_relays: Vec<(String, String)>,
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
            user_relays: Vec::new(),
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
    /// Transitions to `NmpAppBuilder<StorageSet>`. A projection-consumption
    /// decision (`.declare_consumed_projections` /
    /// `.consume_all_builtin_projections`) is still required before `start()`.
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
    /// This transitions to `NmpAppBuilder<StorageSet>`. A projection-consumption
    /// decision is still required before `start()` (see
    /// `.declare_consumed_projections` / `.consume_all_builtin_projections`).
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

// ── Projection-consumption decision (StorageSet → ProjectionsDeclared) ───────
//
// ADR-0053 DEBT 2: the host MUST make an explicit decision about which Tier-2
// kernel built-in projections it consumes before `start()`. Forgetting is a
// compile error (start() only exists on ProjectionsDeclared). Two ways to
// decide — both advance the typestate:
//   * `.declare_consumed_projections(keys)` — narrow to the declared set.
//   * `.consume_all_builtin_projections()`  — explicit "I want everything".

impl NmpAppBuilder<StorageSet> {
    /// Declare the static set of Tier-2 kernel built-in projection keys this
    /// app consumes, and advance to `ProjectionsDeclared` (unlocking `start()`).
    ///
    /// The kernel then serializes ONLY these built-ins (plus any Tier-1
    /// host-registered projections, which self-gate by registration) into each
    /// pushed `SnapshotFrame` — the ADR-0053 narrowing optimization. Keys
    /// accumulate with any added earlier via the `AppHost` trait method (e.g.
    /// by a protocol crate during `register`), since the underlying
    /// `SnapshotRegistry` declaration is additive.
    ///
    /// This is the narrowing path. To opt into the full firehose explicitly
    /// (e.g. a TUI/desktop diagnostic shell that genuinely reads everything),
    /// call [`Self::consume_all_builtin_projections`] instead.
    #[must_use]
    pub fn declare_consumed_projections<I, K>(self, keys: I) -> NmpAppBuilder<ProjectionsDeclared>
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        // SAFETY: `self.app` non-null (builder invariant); not yet started.
        let app: &NmpApp = unsafe { &*self.app };
        NmpApp::declare_consumed_projections(app, keys);
        self.into_projections_declared()
    }

    /// Explicitly opt into receiving ALL Tier-2 kernel built-in projections
    /// (the full firehose — no narrowing), and advance to `ProjectionsDeclared`
    /// (unlocking `start()`).
    ///
    /// This is the **visible, greppable** "I want everything" choice. It sets
    /// the kernel's declared set to the explicit `DeclaredProjections::All`
    /// state (ADR-0053 / Workstream-E4: `All` is the ONE non-footgun way to mean
    /// "every Tier-2 built-in"). Use it only when the host genuinely consumes
    /// the full set (diagnostics shells, TUIs, tests). Production app shells
    /// should prefer [`Self::declare_consumed_projections`] to avoid serializing
    /// built-ins no screen reads (e.g. `relay_diagnostics`).
    ///
    /// The distinction from the old silent default is the whole point: omission
    /// no longer compiles (the typestate), and "everything" is now an explicit
    /// `All`, not a silent empty/undeclared set.
    #[must_use]
    pub fn consume_all_builtin_projections(self) -> NmpAppBuilder<ProjectionsDeclared> {
        // SAFETY: `self.app` non-null (builder invariant); not yet started.
        let app: &NmpApp = unsafe { &*self.app };
        // Explicit `All` — NOT a silent empty set. An undeclared app is the loud
        // forgotten-wiring footgun at `nmp_app_start`; this records the
        // deliberate "I consume everything" intent.
        app.consume_all_builtin_projections();
        self.into_projections_declared()
    }

    /// Internal: move ownership of the inner `NmpApp` + relays into a
    /// `ProjectionsDeclared` builder WITHOUT running `Drop` (same `ptr::read` +
    /// `mem::forget` rationale as the storage transitions — see `storage_path`).
    fn into_projections_declared(self) -> NmpAppBuilder<ProjectionsDeclared> {
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

// ── Initial-relay decision (ProjectionsDeclared → RelaysDeclared) ────────────
//
// #1493: operator relay URLs are leaf-app policy, never an NMP default — not in
// `nmp-core`, not in `nmp-defaults`. The app MUST decide its initial relay set
// before `start()`. Forgetting is a compile error (`start()` only exists on
// `RelaysDeclared`). Two ways to decide — both advance the typestate:
//   * `.with_relays(iter)`        — declare the relay set the app starts with.
//   * `.without_initial_relays()` — explicit "this app ships no built-in relays".

impl NmpAppBuilder<ProjectionsDeclared> {
    /// Declare the initial relay set the app starts with, and advance to
    /// `RelaysDeclared` (unlocking `start()`).
    ///
    /// Each item is a `(url, role)` pair where `role` is a relay-role string
    /// (`"read"`, `"write"`, `"both"`, `"indexer"`, or a composite like
    /// `"both,indexer"`); the kernel canonicalizes it when the row is seeded.
    /// These values are leaf-app policy (#1493) — NMP supplies no default, so
    /// what the app declares here is exactly what the kernel starts with.
    ///
    /// # Panics
    ///
    /// Panics if `relays` is empty — that is the `.without_initial_relays()`
    /// case, which must be chosen explicitly so a no-relay start is never a
    /// silent accident.
    #[must_use]
    pub fn with_relays(
        mut self,
        relays: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> NmpAppBuilder<RelaysDeclared> {
        self.user_relays = relays
            .into_iter()
            .map(|(url, role)| (url.into(), role.into()))
            .collect();
        assert!(
            !self.user_relays.is_empty(),
            "with_relays called with an empty set — use .without_initial_relays() \
             to start with no relays explicitly"
        );
        self.into_relays_declared()
    }

    /// Explicitly start with NO initial relays, and advance to `RelaysDeclared`
    /// (unlocking `start()`).
    ///
    /// This is the visible, greppable opt-out for offline/test/local apps. The
    /// kernel starts with an empty `configured_relays`; network operations
    /// fail-closed (`NoTargets`) until relays are added at runtime via
    /// `nmp_app_add_relay`. Use it only when the app genuinely ships no
    /// built-in relays — otherwise declare them with [`Self::with_relays`].
    #[must_use]
    pub fn without_initial_relays(mut self) -> NmpAppBuilder<RelaysDeclared> {
        self.user_relays = Vec::new();
        self.into_relays_declared()
    }

    /// Internal: move ownership into a `RelaysDeclared` builder WITHOUT running
    /// `Drop` (same `ptr::read` + `mem::forget` rationale as the other
    /// transitions — see `storage_path`).
    fn into_relays_declared(self) -> NmpAppBuilder<RelaysDeclared> {
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

// ── Terminal transition: start (RelaysDeclared only) ─────────────────────────

impl NmpAppBuilder<RelaysDeclared> {
    /// Consume the builder and start the NMP kernel.
    ///
    /// This is the **only** path from `NmpAppBuilder<ProjectionsDeclared>` to a
    /// live `*mut NmpApp`. It:
    ///
    /// 1. Calls `nmp_app_start` with the given `RunConfig`.
    /// 2. Releases ownership of the `NmpApp` pointer to the caller.
    ///
    /// `start()` is reachable ONLY after a projection-consumption decision
    /// (`.declare_consumed_projections` or `.consume_all_builtin_projections`)
    /// — ADR-0053 DEBT 2's compile-time enforcement. After this call, the
    /// builder is gone — no setter is reachable (compile error). The returned
    /// pointer is owned by the caller; free it with `nmp_ffi::nmp_app_free`.
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

        // The app's declared initial relay set (#1493): exactly what
        // `.with_relays(...)` declared, or empty if `.without_initial_relays()`
        // was chosen. NMP carries no relay fallback.
        let relay_defaults: Vec<(String, String)> = user_relays;

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

        // ADR-0053 DEBT 2: by the time we reach `start()` the host has ALREADY
        // made an explicit projection-consumption decision — the typestate
        // guarantees it (`ProjectionsDeclared` is only reachable via
        // `.declare_consumed_projections` or `.consume_all_builtin_projections`).
        // No runtime check is needed on the builder path. The complementary
        // `tracing::warn!` in `nmp_app_start` (nmp-ffi) is the backstop for the
        // raw C-ABI path (Swift/Kotlin), which is outside Rust's type system.

        // SAFETY: `app` is non-null (builder invariant).
        nmp_app_start(app, config.visible_limit, config.emit_hz);
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
    let status = nmp_ffi::nmp_app_set_storage_path(app, c_path.as_ptr());
    debug_assert_eq!(
        status,
        nmp_ffi::NmpConfigStatus::Ok.code(),
        "builder storage_path must run before nmp_app_start"
    );
}
