//! `NmpAppBuilder` — typestate-guarded composition root for NMP-based apps.
//!
//! # V-94 — compile-time enforcement of pre-start ordering
//!
//! The problem: every wiring setter (`set_routing_substrate`,
//! `register_action`, …) must run **before** [`NmpApp::start_runtime`] — the
//! actor reads all wiring slots once at kernel construction; a later setter is
//! silently ignored (D6). The **consume-and-return typestate** enforces this in
//! Rust: `start(self, config)` moves the builder, so no setter is reachable
//! post-start. C ABI wrappers still expose runtime late-wiring diagnostics for
//! raw pointer callers outside Rust's type system.
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
//! (2) an ADR-0070 projection-consumption decision
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
//! use nmp_native_runtime::{NmpApp, NmpAppBuilder, RunConfig};
//!
//! let app: *mut NmpApp = NmpAppBuilder::new()
//!     .in_memory()                                  // required: choose storage
//!     .declare_consumed_projections(["profile"])    // required: declare ADR-0070 set
//!     .with_relays([("wss://your.relay", "both")])  // required: declare relays (#1493)
//!     .start(RunConfig::default());                 // consume builder → started handle
//!
//! // `NmpAppBuilder` is gone; setters are unreachable.
//! // The caller now owns the returned native runtime handle.
//! ```
//!
//! The canonical production step replaces `.in_memory()` with
//! `.storage_path("/path/to/lmdb/dir")`. A diagnostics shell that genuinely
//! reads every built-in calls `.consume_all_builtin_projections()` in place of
//! `.declare_consumed_projections(...)`.
//!
//! # Scope
//!
//! This type lives in `nmp-native-runtime` and targets Rust composition roots.
//! C ABI crates wrap the started handle; Swift/Kotlin shells stay unchanged.
//!
//! [`AppHost`]: nmp_core::substrate::AppHost

use std::marker::PhantomData;

use crate::{new_app, NmpApp};

mod app_host_impl; // ADR-0070: `impl AppHost for NmpAppBuilder` child submodule (LOC ceiling).
#[cfg(feature = "marmot")]
mod marmot;
mod transitions;
#[cfg(feature = "wallet")]
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
/// projection-consumption decision must be made first (ADR-0070 DEBT 2):
/// call `.declare_consumed_projections(keys)` to narrow to a set, or
/// `.consume_all_builtin_projections()` to explicitly opt into the full
/// Tier-2 firehose.
pub struct StorageSet;

/// Builder state: the host has made an explicit projection-consumption
/// decision (ADR-0070 DEBT 2).
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
/// forgetting to decide. NMP (including `explicit composition`) owns no operator relay
/// URLs; the relay set is leaf-app policy supplied here.
pub struct RelaysDeclared;

// ── RunConfig ────────────────────────────────────────────────────────────────

/// Runtime configuration forwarded to [`NmpApp::start_runtime`].
///
/// Mirrors the two runtime start parameters:
/// `visible_limit` (max rows the kernel emits per snapshot) and `emit_hz`
/// (snapshot-emission rate).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunConfig {
    /// Maximum number of feed rows the kernel includes in each snapshot.
    /// Forwarded to [`NmpApp::start_runtime`] as `visible_limit`.
    pub visible_limit: u32,
    /// Snapshot-emission rate in Hz. Forwarded to [`NmpApp::start_runtime`] as
    /// `emit_hz`.
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
/// Owns the native `NmpApp` pointer during the wiring phase and
/// guarantees at compile time that:
///
/// 1. All `AppHost`/`ActionRegistrar` setters (action modules, routing
///    substrate, coverage hook, …) run **before** `start()`.
/// 2. A storage decision (`.storage_path` or `.in_memory()`) is made before
///    `start()` — the one slot whose omission causes silent data loss.
/// 3. An ADR-0070 projection-consumption decision
///    (`.declare_consumed_projections(keys)` or
///    `.consume_all_builtin_projections()`) is made before `start()` — so the
///    full Tier-2 built-in firehose can never be shipped silently.
/// 4. `start()` is callable **exactly once** (it moves `self`).
///
/// On `Drop`, if `start()` was never called, the inner `NmpApp` is dropped to
/// prevent a memory leak.
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
/// use nmp_native_runtime::{NmpAppBuilder, RunConfig};
///
/// // ERROR: no method named `start` found for `NmpAppBuilder<Unstarted>`
/// let _app = NmpAppBuilder::new().start(RunConfig::default());
/// ```
///
/// # Compile-fail: calling `start()` without a projection decision is an error
///
/// The following code does **not** compile because `start()` only exists on
/// `NmpAppBuilder<ProjectionsDeclared>`, not on `NmpAppBuilder<StorageSet>` —
/// ADR-0070 DEBT 2's compile-time enforcement that the host cannot silently
/// ship the full Tier-2 built-in firehose:
///
/// ```compile_fail
/// use nmp_native_runtime::{NmpAppBuilder, RunConfig};
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
/// use nmp_native_runtime::{NmpAppBuilder, RunConfig};
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
/// use nmp_native_runtime::{NmpAppBuilder, RunConfig};
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
// native-runtime side). The builder's raw pointer is owned exclusively by the
// builder instance; no alias exists until `start()` returns it.
// `PhantomData<S>` is always `Send + Sync` for our ZST markers.
unsafe impl<S> Send for NmpAppBuilder<S> {}
unsafe impl<S> Sync for NmpAppBuilder<S> {}

impl NmpAppBuilder<Unstarted> {
    /// Allocate a fresh `NmpApp` and enter the wiring phase.
    ///
    /// Constructs a native runtime handle for the wiring phase.
    pub fn new() -> Self {
        let app = Box::into_raw(Box::new(new_app()));
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
