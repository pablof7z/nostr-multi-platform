//! ADR-0070 / Workstream-E4 — the `NmpApp` projection-consumption-intent surface.
//!
//! Extracted from `lib.rs` (god-file relief): the inherent `NmpApp` methods that
//! set / read the host-declared consumed-projection intent. The matching C-ABI
//! seams live in [`crate::snapshot`]
//! (`nmp_app_declare_consumed_projections` / `nmp_app_consume_all_builtin_projections`).

use std::sync::atomic::Ordering;

use crate::NmpApp;

impl NmpApp {
    /// ADR-0070 — declare (union into) the static set of Tier-2 built-in
    /// projection keys this host consumes (the narrowing path).
    ///
    /// The output-side sibling of relay interest installs: the kernel serializes a
    /// kernel-owned built-in into each snapshot only if its key is declared. A
    /// non-empty set narrows to its members, skipping the producer work (notably
    /// the `relay_diagnostics` roll-up) for everything else. Additive, `&self`
    /// (lock-and-extend), intended as a host-init call before `nmp_app_start`.
    /// A poisoned registry mutex is a silent no-op (D6). To consume the full set,
    /// call [`Self::consume_all_builtin_projections`] instead (leaving the intent
    /// undeclared is a loud forgotten-wiring bug, not a silent firehose).
    ///
    /// # Init-only invariant (ADR-0070 Decision 5)
    ///
    /// The declared set MUST be written before the first real frame is emitted
    /// (i.e. before `nmp_app_start`). A `debug_assert!` enforces this in debug
    /// builds; additive-only semantics keep a post-start call safe in release.
    pub fn declare_consumed_projections<I, K>(&self, keys: I)
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        debug_assert!(
            !self.started.load(Ordering::SeqCst),
            "declare_consumed_projections called after nmp_app_start — \
             the declared set must be complete before the kernel emits its \
             first real frame (ADR-0070 Decision 5 / init-only invariant)"
        );
        if let Ok(mut registry) = self.snapshot_projections.lock() {
            registry.declare_consumed_projections(keys);
        }
    }

    /// ADR-0070 / Workstream-E4 — declare the explicit "I consume every Tier-2
    /// built-in" intent (`DeclaredProjections::All`).
    ///
    /// This is the ONE non-footgun way to receive the full built-in set. Full
    /// Rust clients (chirp-tui / chirp-desktop) and the Chirp shells call it
    /// (via `nmp_app_consume_all_builtin_projections`) instead of relying on a
    /// silent permissive default. Idempotent; a host-init call before
    /// `nmp_app_start`. A poisoned registry mutex is a silent no-op (D6). Like
    /// [`Self::declare_consumed_projections`], a debug build asserts it happens
    /// before `nmp_app_start` (ADR-0070 Decision 5).
    pub fn consume_all_builtin_projections(&self) {
        debug_assert!(
            !self.started.load(Ordering::SeqCst),
            "consume_all_builtin_projections called after nmp_app_start — \
             the consumption intent must be set before the kernel emits its \
             first real frame (ADR-0070 Decision 5 / init-only invariant)"
        );
        if let Ok(mut registry) = self.snapshot_projections.lock() {
            registry.consume_all_builtin_projections();
        }
    }

    /// ADR-0070 / Workstream-E4 — whether the host expressed NO
    /// projection-consumption intent (the loud forgotten-declaration state).
    ///
    /// `nmp_app_start` reads this to fire the `debug_assert!` + non-fatal
    /// `tracing::warn!`. An explicit `consume_all_builtin_projections()` (`All`)
    /// is NOT undeclared and never warns. A poisoned registry mutex returns
    /// `false` (D6 — degrade quietly, never false-fire the assert).
    #[must_use]
    pub fn consumed_projections_are_undeclared(&self) -> bool {
        self.snapshot_projections
            .lock()
            .map(|r| r.declared_projections().is_undeclared())
            .unwrap_or(false)
    }

    /// ADR-0070 — whether the host narrowed to an explicit subset of built-ins.
    ///
    /// `false` for both `Undeclared` and the explicit `All` (neither narrows).
    /// Read by the `explicit composition` builder tests that assert the typestate methods
    /// produce the expected narrowing state. A poisoned registry mutex returns
    /// `false` (D6).
    #[must_use]
    pub fn consumed_projections_are_narrowing(&self) -> bool {
        self.snapshot_projections
            .lock()
            .map(|r| r.declared_projections().is_narrowing())
            .unwrap_or(false)
    }
}
