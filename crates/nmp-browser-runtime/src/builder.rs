//! `BrowserAppBuilder<S>` — typestate composition root for NMP on browser
//! runtimes (issue #2046 / PR-B of the browser-runtime epic #2045).
//!
//! # Typestate ladder (ADR-0053 / ADR-0067)
//!
//! ```text
//! BrowserAppBuilder<Unstarted>          ← constructed by BrowserAppBuilder::new()
//!   │  .inject_store(store)
//!   ▼
//! BrowserAppBuilder<StorageSet>         ← EventStore injected (ADR-0054 §5)
//!   │  .declare_projections(...)         optional; all AppHost setters available
//!   ▼
//! BrowserAppBuilder<ProjectionsDeclared> ← ADR-0053 consumed-projection gate
//!   │  .set_relays(relays)
//!   ▼
//! BrowserAppBuilder<RelaysDeclared>     ← relay bootstrap list provided
//!   │  .decide_providers(config)         ADR-0067 capability-provider gate
//!   ▼
//! BrowserAppBuilder<ProvidersDecided>   ← signer/capability path decided
//!   │  .start()                         → calls register_defaults + wires runtime
//!   ▼
//! BrowserRuntimeHandle                  ← pump-driven runtime handle (#2058)
//! ```
//!
//! Every `AppHost` / `ActionRegistrar` setter is usable in ALL states (they
//! accumulate into `BrowserBuilderInner`); the typestate gates only prevent
//! calling `start()` before the mandatory seams have been satisfied.
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

use std::sync::Mutex;

use crate::runtime::BrowserRuntimeHandle;

mod state;
pub(crate) use state::BrowserBuilderInner;

mod app_host_impl;

// ── Typestate markers ─────────────────────────────────────────────────────────

/// Stage 0: builder freshly constructed; no store injected yet.
#[non_exhaustive]
pub struct Unstarted;

/// Stage 1: `EventStore` injected (ADR-0054 §5 seam).
#[non_exhaustive]
pub struct StorageSet;

/// Stage 2: consumed-projection set declared (ADR-0053 gate).
#[non_exhaustive]
pub struct ProjectionsDeclared;

/// Stage 3: relay bootstrap list provided.
#[non_exhaustive]
pub struct RelaysDeclared;

/// Stage 4: signer / capability providers decided (ADR-0067 gate).
/// `start()` is only available in this state.
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
}

impl Default for BrowserAppBuilder<Unstarted> {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserAppBuilder<StorageSet> {
    /// Declare the set of Tier-2 built-in projection keys this host consumes
    /// (ADR-0053 gate). Pass an empty iterator to receive every built-in
    /// (no narrowing).
    pub fn declare_projections<I, K>(self, keys: I) -> BrowserAppBuilder<ProjectionsDeclared>
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        {
            let Ok(g) = self.inner.lock() else {
                return self.advance();
            };
            g.reducer.declare_consumed_projections(keys);
        }
        self.advance()
    }
}

impl BrowserAppBuilder<ProjectionsDeclared> {
    /// Provide the relay bootstrap list `[(url, role)]`.
    ///
    /// Roles are normalised by the kernel's relay-role parser (same as the
    /// native actor). The list is applied to the reducer at `start()` after
    /// the store has been fully injected.
    pub fn set_relays(self, relays: Vec<(String, String)>) -> BrowserAppBuilder<RelaysDeclared> {
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
}

impl BrowserAppBuilder<RelaysDeclared> {
    /// Declare the signer / capability-provider decision (ADR-0067 gate).
    ///
    /// `config` carries any runtime parameters the browser signer setup needs.
    /// After this call `start()` becomes available.
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
}

// ── BrowserRunConfig ──────────────────────────────────────────────────────────

/// Runtime configuration provided at the `decide_providers` gate (ADR-0067).
///
/// Carried into `BrowserRuntime` to supply platform-specific parameters the
/// signer / capability layer needs at start time. Currently a forward-
/// compatible empty struct; future fields (e.g. `signer_kind`, browser
/// extension origin) will be added here without breaking the gate API.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct BrowserRunConfig {
    /// Optional app identifier (used for per-app storage namespacing on
    /// OPFS-SQLite backends). Empty string → no namespacing.
    pub app_id: String,
}
