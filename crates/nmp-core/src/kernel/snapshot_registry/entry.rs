//! [`ProjectionEntry`] — the per-key closure + change-gate memo machinery.
//!
//! Extracted from `snapshot_registry.rs` so the registry file stays within its
//! LOC ceiling. This is the gate/memo fast-path that lets `run` skip
//! re-invoking a projection closure when its change-gate is unchanged (see
//! [`super::ChangeGate`]), plus the D15 `catch_unwind` boundary that contains a
//! panicking host closure.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};

use super::{ChangeGate, ProjectionFn};

/// A registered generic projection: the closure plus the optional change-gate
/// memo that lets `run` skip re-invoking the closure when the gate is unchanged.
///
/// - `f` — the host projection closure (always present).
/// - `gate` — `None` for the default always-run registration
///   ([`super::SnapshotRegistry::register`]); `Some` for the gated variant
///   ([`super::SnapshotRegistry::register_gated`]).
/// - `memo` — interior-mutable per-key cache of `(last witnessed gate value,
///   last produced value)`. Interior mutability (a `Mutex`) is required because
///   [`super::SnapshotRegistry::run`] takes `&self` (it is driven from
///   `make_update` through a shared `&self` kernel path); threading `&mut` all
///   the way through `make_update` would ripple a borrow change across the whole
///   emit path. The `Mutex` is contended only by the single actor thread that
///   drives `run`, so it is effectively uncontended in production. `None` until
///   the first run populates it; ignored entirely when `gate` is `None`.
pub(super) struct ProjectionEntry {
    f: ProjectionFn,
    gate: Option<Arc<dyn ChangeGate>>,
    memo: Mutex<Option<(u64, serde_json::Value)>>,
}

impl ProjectionEntry {
    /// An ungated (always-run) entry — the default registration semantics.
    pub(super) fn ungated(f: ProjectionFn) -> Self {
        Self {
            f,
            gate: None,
            memo: Mutex::new(None),
        }
    }

    /// A gated entry — `run` consults `gate` and may serve `memo` instead of
    /// invoking `f`.
    pub(super) fn gated(gate: Arc<dyn ChangeGate>, f: ProjectionFn) -> Self {
        Self {
            f,
            gate: Some(gate),
            memo: Mutex::new(None),
        }
    }

    /// Produce this entry's value for the current tick.
    ///
    /// Returns `Some(value)` on success; `None` when the closure panicked (D15:
    /// every host-supplied closure invocation is wrapped in [`catch_unwind`] so
    /// a panicking projection can never unwind the actor thread). The caller
    /// ([`super::SnapshotRegistry::run`]) omits the key from the snapshot when
    /// `None` is returned, exactly as if the key had never been registered.
    ///
    /// Ungated: always invokes the closure (legacy semantics, unchanged).
    /// Gated: if the gate value matches the memoized value, clones and returns
    /// the cached value WITHOUT invoking the closure; otherwise invokes the
    /// closure, caches `(gate_value, value)`, and returns it. A panicking
    /// gated closure leaves the prior memo intact (not overwritten), so the
    /// next clean gate still serves the last good cached value.
    pub(super) fn value_for_tick(&self) -> Option<serde_json::Value> {
        let Some(gate) = self.gate.as_ref() else {
            // Ungated: the default always-run path — never touches the memo.
            // D15: host-supplied closure invocation wrapped in catch_unwind.
            return catch_unwind(AssertUnwindSafe(|| (self.f)())).ok();
        };

        let gate_value = gate.current();
        // Fast path: a clean gate serves the cached value without invoking `f`.
        // The memo mutex is contended only by the single actor thread, so this
        // lock is effectively uncontended. A poisoned memo (defensive — the lock
        // is always released before `f` runs, so a closure panic can never
        // poison it) collapses to a fresh run.
        if let Ok(memo) = self.memo.lock() {
            if let Some((cached_gate, cached_value)) = memo.as_ref() {
                if *cached_gate == gate_value {
                    return Some(cached_value.clone());
                }
            }
        }

        // Dirty (or never run): invoke `f` inside catch_unwind (D15), then
        // memoize for the next tick on success. `f` runs OUTSIDE the memo lock
        // so a slow/panicking closure never holds the memo mutex. On panic,
        // return `None` (key omitted this tick) and leave the prior memo intact.
        let value = match catch_unwind(AssertUnwindSafe(|| (self.f)())) {
            Ok(v) => v,
            Err(_) => return None,
        };
        if let Ok(mut memo) = self.memo.lock() {
            *memo = Some((gate_value, value.clone()));
        }
        Some(value)
    }
}
