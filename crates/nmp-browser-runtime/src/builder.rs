//! `BrowserAppBuilder<S>` — typestate composition root for NMP on browser
//! runtimes (issue #2046 / PR-B of the browser-runtime epic #2045).
//!
//! # Typestate ladder (ADR-0053 / ADR-0067, mirrors native `NmpAppBuilder`)
//!
//! ```text
//! BrowserAppBuilder<Unstarted>          ← constructed by BrowserAppBuilder::new()
//!   │  .inject_store(store)  ─┐  explicit storage choice (no silent default)
//!   │  .in_memory()          ─┤
//!   ▼                          ▼
//! BrowserAppBuilder<StorageSet>         ← EventStore decided (ADR-0054 §5)
//!   │  .declare_projections(...)         ─┐ explicit ADR-0053 projection decision
//!   │  .consume_all_builtin_projections()─┤
//!   ▼                                      ▼
//! BrowserAppBuilder<ProjectionsDeclared> ← ADR-0053 consumed-projection gate
//!   │  .set_relays(relays)         ─┐ explicit #1493 relay decision (non-empty)
//!   │  .without_initial_relays()   ─┤
//!   ▼                                ▼
//! BrowserAppBuilder<RelaysDeclared>     ← relay decision made
//!   │  .decide_providers(config)         ADR-0067 explicit no-providers-yet gate
//!   ▼
//! BrowserAppBuilder<ProvidersDecided>   ← capability/signer decision recorded
//!   │  .start()                         → calls register_defaults + wires runtime
//!   ▼
//! BrowserRuntimeHandle                  ← pump-driven runtime handle (#2058)
//! ```
//!
//! Each transition has TWO twins so the decision is always explicit and
//! greppable (never a silent default), mirroring native `NmpAppBuilder`:
//! storage (`inject_store` / `in_memory`), ADR-0053 projections
//! (`declare_projections` / `consume_all_builtin_projections`), and #1493
//! relays (`set_relays` / `without_initial_relays`). Every `AppHost` /
//! `ActionRegistrar` setter is usable in ALL states (they accumulate into
//! `BrowserBuilderInner`); the typestate gates only prevent calling `start()`
//! before the mandatory decisions have been made.
//!
//! # Design
//!
//! `BrowserAppBuilder<S>` wraps `Mutex<BrowserBuilderInner>` for interior
//! mutability: `AppHost` registrar methods take `&self` (they lock the mutex);
//! `ActionRegistrar` takes `&mut self` (same mutex, exclusive). On wasm32 the
//! Mutex is always uncontested (single-threaded target). The builder is
//! `Send + Sync` so it can be pre-built on any thread and handed to the wasm
//! binding.
//!
//! # Issue #2058 — hide raw reducer/runtime handles
//!
//! `BrowserRuntimeHandle` (returned by `start()`) exposes only the narrow
//! dispatch / snapshot / sign surface (#2058). The raw `KernelReducer` and
//! `BrowserRuntime` are NOT public.

use std::sync::{Arc, Mutex};

use nmp_core::Clock;

use crate::runtime::BrowserRuntimeHandle;

mod state;
pub(crate) use state::BrowserBuilderInner;

mod app_host_impl;

// ── Typestate markers ─────────────────────────────────────────────────────────

/// Stage 0: builder freshly constructed; no storage decision made yet.
/// Advance via `.inject_store(store)` or `.in_memory()`.
#[non_exhaustive]
pub struct Unstarted;

/// Stage 1: storage explicitly chosen (`inject_store` or `in_memory`,
/// ADR-0054 §5 seam). A projection-consumption decision is still required.
#[non_exhaustive]
pub struct StorageSet;

/// Stage 2: an explicit ADR-0053 projection decision was made
/// (`declare_projections` narrowing, or `consume_all_builtin_projections`).
#[non_exhaustive]
pub struct ProjectionsDeclared;

/// Stage 3: an explicit #1493 initial-relay decision was made
/// (`set_relays` non-empty, or `without_initial_relays`).
#[non_exhaustive]
pub struct RelaysDeclared;

/// Stage 4: the ADR-0067 capability/signer-provider decision was recorded
/// (`decide_providers`). `start()` is only available in this state.
#[non_exhaustive]
pub struct ProvidersDecided;

// ── Builder struct ────────────────────────────────────────────────────────────

/// Browser-platform NMP composition root.
///
/// Wraps a `KernelReducer` and accumulates the full `AppHost` registration
/// surface through a `Mutex<BrowserBuilderInner>`. Calling `.start()` on
/// `BrowserAppBuilder<ProvidersDecided>` applies all deferred settings,
/// calls `nmp_defaults::register_defaults`, and returns a
/// `BrowserRuntimeHandle`.
///
/// # Example
///
/// ```ignore
/// let handle = BrowserAppBuilder::new()
///     .inject_store(my_store)
///     .declare_projections(["nmp.chirp.feed", "nmp.profile"])
///     .set_relays(vec![("wss://relay.damus.io".into(), "read-write".into())])
///     .decide_providers(BrowserRunConfig::default())
///     .start();
/// handle.pump();
/// ```
///
/// # Compile-fail: `start()` is unavailable before each gate
///
/// `start()` exists ONLY on `BrowserAppBuilder<ProvidersDecided>`. Skipping any
/// gate is a compile error:
///
/// ```compile_fail
/// use nmp_browser_runtime::BrowserAppBuilder;
/// // ERROR: no method named `start` on BrowserAppBuilder<Unstarted>
/// let _ = BrowserAppBuilder::new().start();
/// ```
///
/// ```compile_fail
/// use nmp_browser_runtime::BrowserAppBuilder;
/// // ERROR: no `start` on StorageSet (projection decision missing)
/// let _ = BrowserAppBuilder::new().in_memory().start();
/// ```
///
/// ```compile_fail
/// use nmp_browser_runtime::BrowserAppBuilder;
/// // ERROR: no `start` on ProjectionsDeclared (relay decision missing)
/// let _ = BrowserAppBuilder::new()
///     .in_memory()
///     .consume_all_builtin_projections()
///     .start();
/// ```
///
/// ```compile_fail
/// use nmp_browser_runtime::BrowserAppBuilder;
/// // ERROR: no `start` on RelaysDeclared (provider decision missing)
/// let _ = BrowserAppBuilder::new()
///     .in_memory()
///     .consume_all_builtin_projections()
///     .without_initial_relays()
///     .start();
/// ```
pub struct BrowserAppBuilder<S> {
    pub(crate) inner: Mutex<BrowserBuilderInner>,
    _state: std::marker::PhantomData<S>,
}

impl BrowserAppBuilder<Unstarted> {
    /// Construct a fresh builder in `Unstarted` state.
    ///
    /// Allocates a new `KernelReducer` and wires the mailbox channel + relay
    /// slot. All `AppHost` setters may be called immediately; they accumulate
    /// into the inner state until `start()`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(BrowserBuilderInner::new()),
            _state: std::marker::PhantomData,
        }
    }

    /// Inject the platform's `EventStore` (ADR-0054 §5 seam).
    ///
    /// The store is swapped into the `KernelReducer` at this gate so
    /// subsequent `AppHost` registrar calls can resolve against the real store
    /// (e.g. `register_search_scope` → `SearchScopeRegistry::install_into`).
    ///
    /// This is the persistent / platform-backed storage choice (the future
    /// OPFS-SQLite browser backend, #1007). For an explicit ephemeral store use
    /// [`Self::in_memory`].
    pub fn inject_store(
        self,
        store: std::sync::Arc<dyn nmp_store::EventStore>,
    ) -> BrowserAppBuilder<StorageSet> {
        {
            let Ok(mut g) = self.inner.lock() else {
                // D6: lock poisoned — advance the state anyway; the missing store
                // will surface as an empty reducer during start().
                return self.advance();
            };
            g.reducer.replace_store_for_start(store);
        }
        self.advance()
    }

    /// Use an ephemeral in-memory store (explicit opt-in).
    ///
    /// The freshly-constructed `KernelReducer` already holds an in-memory store;
    /// this method makes that choice **explicit and greppable** (mirroring native
    /// `NmpAppBuilder::in_memory`) rather than letting an un-injected store be a
    /// silent default. An in-memory store loses all events when the page/Worker
    /// is torn down — suitable for tests and short-lived tools. For production,
    /// inject a persistent store via [`Self::inject_store`].
    pub fn in_memory(self) -> BrowserAppBuilder<StorageSet> {
        // No store swap — the default reducer store IS in-memory. The explicit
        // call is the whole point: the storage decision is now visible in code.
        self.advance()
    }
}

impl Default for BrowserAppBuilder<Unstarted> {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserAppBuilder<StorageSet> {
    /// Declare the set of Tier-2 built-in projection keys this host consumes
    /// (ADR-0053 narrowing gate), and advance to `ProjectionsDeclared`.
    ///
    /// This is the **narrowing** path: the kernel serializes only the declared
    /// built-ins (plus Tier-1 host/protocol projections, which self-gate by
    /// registration). Keys accumulate with any declared earlier via the
    /// `AppHost` trait method.
    ///
    /// # Panics
    ///
    /// Panics if `keys` is empty — that is the [`Self::consume_all_builtin_projections`]
    /// case, which must be chosen explicitly so a no-op narrowing is never a
    /// silent ADR-0053 footgun. An empty `declare_projections([])` advances the
    /// typestate but declares nothing, leaving `DeclaredProjections::Undeclared`
    /// — the loud forgotten-declaration state. To receive every built-in, call
    /// [`Self::consume_all_builtin_projections`] instead.
    pub fn declare_projections<I, K>(self, keys: I) -> BrowserAppBuilder<ProjectionsDeclared>
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        let keys_vec: Vec<String> = keys.into_iter().map(Into::into).collect();
        assert!(
            !keys_vec.is_empty(),
            "declare_projections called with an empty set — use \
             .consume_all_builtin_projections() to opt into the full Tier-2 firehose \
             explicitly (#2072: an empty narrowing is the ADR-0053 footgun)"
        );
        {
            let Ok(g) = self.inner.lock() else {
                return self.advance();
            };
            g.reducer.declare_consumed_projections(keys_vec);
        }
        self.advance()
    }

    /// Explicitly opt into receiving ALL Tier-2 kernel built-in projections (the
    /// full firehose — no narrowing), and advance to `ProjectionsDeclared`.
    ///
    /// This is the visible, greppable "I want everything" choice (mirrors native
    /// `NmpAppBuilder::consume_all_builtin_projections`). It sets the kernel's
    /// declared set to the explicit `DeclaredProjections::All` state — never a
    /// silent empty default. Use it for diagnostics shells / TUIs / tests that
    /// genuinely read the full set; production shells should prefer
    /// [`Self::declare_projections`] to avoid serializing unused built-ins.
    pub fn consume_all_builtin_projections(self) -> BrowserAppBuilder<ProjectionsDeclared> {
        {
            let Ok(g) = self.inner.lock() else {
                return self.advance();
            };
            g.reducer.consume_all_builtin_projections();
        }
        self.advance()
    }
}

impl BrowserAppBuilder<ProjectionsDeclared> {
    /// Provide the initial relay bootstrap list `[(url, role)]`, and advance to
    /// `RelaysDeclared`.
    ///
    /// Roles are normalised by the kernel's relay-role parser (same as the
    /// native actor). The list is applied to the reducer at `start()` after the
    /// store has been fully injected. These values are leaf-app policy (#1493) —
    /// NMP supplies no relay default, so what is declared here is exactly what
    /// the kernel starts with.
    ///
    /// # Panics
    ///
    /// Panics if `relays` is empty — that is the [`Self::without_initial_relays`]
    /// case, which must be chosen explicitly so a no-relay start is never a
    /// silent accident (mirrors native `NmpAppBuilder::with_relays`, #1493).
    pub fn set_relays(self, relays: Vec<(String, String)>) -> BrowserAppBuilder<RelaysDeclared> {
        assert!(
            !relays.is_empty(),
            "set_relays called with an empty set — use .without_initial_relays() \
             to start with no relays explicitly (#1493: NMP has no relay default)"
        );
        {
            let Ok(mut g) = self.inner.lock() else {
                return self.advance();
            };
            // Store for deferred application in start() (after all `&mut`
            // seams have been applied).
            g.relay_bootstrap = relays;
        }
        self.advance()
    }

    /// Explicitly start with NO initial relays, and advance to `RelaysDeclared`.
    ///
    /// The visible, greppable opt-out (mirrors native
    /// `NmpAppBuilder::without_initial_relays`, #1493). The kernel starts with an
    /// empty configured-relay set; network operations fail-closed (`NoTargets`)
    /// until relays are added at runtime via the command inbox. Use it only when
    /// the app genuinely ships no built-in relays — otherwise declare them with
    /// [`Self::set_relays`].
    pub fn without_initial_relays(self) -> BrowserAppBuilder<RelaysDeclared> {
        {
            let Ok(mut g) = self.inner.lock() else {
                return self.advance();
            };
            g.relay_bootstrap = Vec::new();
        }
        self.advance()
    }
}

impl BrowserAppBuilder<RelaysDeclared> {
    /// Record the explicit capability/signer-provider decision (ADR-0067 gate),
    /// and advance to `ProvidersDecided` (unlocking `start()`).
    ///
    /// This gate makes the provider decision a **required, explicit step** before
    /// `start()` — a runtime cannot silently boot with an undeclared capability
    /// posture. Today it records the honest **"no providers wired yet"** decision:
    /// `config` carries the run parameters, and the concrete signer/capability
    /// providers (NIP-07, NIP-46, local-key) are registered by the browser
    /// capability + signer-provider registry in #2049. When that lands, the
    /// provider set is supplied through `config` here; the gate itself stays.
    pub fn decide_providers(
        self,
        config: BrowserRunConfig,
    ) -> BrowserAppBuilder<ProvidersDecided> {
        {
            let Ok(mut g) = self.inner.lock() else {
                return self.advance();
            };
            g.run_config = Some(config);
        }
        self.advance()
    }
}

impl BrowserAppBuilder<ProvidersDecided> {
    /// Finalise composition and return a `BrowserRuntimeHandle`.
    ///
    /// Sequence:
    /// 1. Call `nmp_defaults::register_defaults(self)` — wires substrate,
    ///    action modules, and all NMP defaults.
    /// 2. Apply all deferred `&mut`-kernel settings (routing, coverage hook,
    ///    publish resolver, relay slot, etc.).
    /// 3. Install the search-scope registry into the event store.
    /// 4. Apply the relay bootstrap list.
    /// 5. Build `BrowserRuntime` and wrap in `BrowserRuntimeHandle`.
    pub fn start(mut self) -> BrowserRuntimeHandle {
        // Step 1 — register NMP defaults (takes `&mut impl AppHost`).
        nmp_defaults::register_defaults(&mut self);

        // Step 2 — consume the inner state and build the runtime.
        let inner = self
            .inner
            .into_inner()
            .expect("BrowserAppBuilder::start: inner Mutex poisoned");

        // #2072 — loud ADR-0053 gate: after register_defaults, declared
        // projections MUST be in `All` or `Narrow` state, not `Undeclared`.
        // Fires as a panic in debug/test builds; degrades to a warn in release.
        debug_assert!(
            !inner.reducer.declared_projections_is_undeclared(),
            "BrowserAppBuilder::start(): projection-consumption intent was never \
             declared. Call .declare_projections([...]) or \
             .consume_all_builtin_projections() before start() (ADR-0053 gate, #2072)."
        );
        #[cfg(not(debug_assertions))]
        if inner.reducer.declared_projections_is_undeclared() {
            tracing::warn!(
                "BrowserAppBuilder::start(): projection intent undeclared — \
                 ADR-0053 footgun; call declare_projections or \
                 consume_all_builtin_projections before start() (#2072)"
            );
        }

        BrowserRuntimeHandle::from_builder_inner(inner)
    }
}

// ── Typestate advance helper ──────────────────────────────────────────────────
//
// Used by every gate method to consume `self` and re-wrap in the next state.
// The `inner` Mutex is moved without cloning; the `PhantomData` type parameter
// changes at compile time only (zero cost).

impl<S> BrowserAppBuilder<S> {
    fn advance<NewState>(self) -> BrowserAppBuilder<NewState> {
        BrowserAppBuilder {
            inner: self.inner,
            _state: std::marker::PhantomData,
        }
    }

    // ── #2076 — deterministic clock seam ─────────────────────────────────────

    /// Inject a host-supplied or deterministic kernel clock (#2076).
    ///
    /// The injected `Arc<dyn Clock>` is stored in the builder and applied to
    /// the `KernelReducer` at `start()` via `KernelReducer::set_clock`. Use
    /// this to supply a deterministic replay clock or a host-controlled
    /// wall-clock adapter. The `Clock` trait returns
    /// `nmp_core::time::SystemTime` (`web_time::SystemTime` on wasm32 —
    /// **no** `std::time`).
    ///
    /// NOT a typestate gate — accumulating setter, available in all builder
    /// states. A later call overwrites an earlier one (last-writer-wins).
    pub fn with_clock(self, clock: Arc<dyn Clock>) -> Self {
        if let Ok(mut g) = self.inner.lock() {
            g.clock = Some(clock);
        }
        self
    }

    /// Explicitly opt into the default web-time wall-clock (#2076).
    ///
    /// Makes the "I accept non-deterministic time" decision visible in code
    /// rather than relying on the absence of `.with_clock(...)`. Calling this
    /// clears any previously injected clock (the system clock IS the default —
    /// this is the greppable opt-in). Available in all builder states.
    pub fn with_system_clock(self) -> Self {
        if let Ok(mut g) = self.inner.lock() {
            g.clock = None; // None → default web-time SystemClock at start()
        }
        self
    }
}

// ── BrowserRunConfig ──────────────────────────────────────────────────────────

/// Runtime configuration provided at the `decide_providers` gate (ADR-0067).
///
/// Carried into `BrowserRuntime` to supply platform-specific parameters the
/// signer / capability layer needs at start time. The capability/signer-provider
/// fields (e.g. the chosen NIP-07 / NIP-46 / local-key provider set) are added
/// here by #2049; today the struct carries only the storage-namespacing id, and
/// the gate records the explicit "no providers wired yet" decision.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct BrowserRunConfig {
    /// Optional app identifier (used for per-app storage namespacing on
    /// OPFS-SQLite backends, #1007). Empty string → no namespacing.
    pub app_id: String,
}
