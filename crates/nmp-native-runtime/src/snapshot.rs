//! FFI snapshot-projection registration entry point.
//!
//! Provides the typed (FlatBuffers) registration seam ([`NmpApp::register_typed_snapshot_projection`])
//! and C-ABI declaration surfaces for consumed projections and incremental-apply.
//! The generic (`serde_json::Value`) lane has been removed; all projections use
//! the typed FlatBuffers sidecar (ADR-0072).

use super::NmpApp;
use nmp_core::CompositionLedger;
use nmp_ownership::ProjectionRegistrationKey;
use nmp_core::__ffi_internal::SnapshotProjectionSlot;

/// Register a typed FlatBuffers projection closure directly against the shared
/// snapshot registry + composition ledger slots, without borrowing `NmpApp`.
///
/// This is the ONE canonical registration body (ADR-0069 disposition
/// recording). [`NmpApp::register_typed_snapshot_projection_with_time`]
/// forwards here; the Send [`crate::NmpReadHost`] handle
/// ([`crate::read_host_handle`]) calls it too so a worker thread can install a
/// read-session output after an off-thread resolve (#2927) — one path, no fork.
pub(crate) fn register_typed_snapshot_projection_on(
    snapshot_projections: &SnapshotProjectionSlot,
    composition_ledger: &CompositionLedger,
    key: impl Into<ProjectionRegistrationKey>,
    f: impl Fn(u64) -> Option<nmp_core::TypedProjectionData> + Send + Sync + 'static,
) {
    use nmp_core::__ffi_internal::TypedAdmission;
    let key = key.into().into_string();
    if let Ok(mut registry) = snapshot_projections.lock() {
        // ADR-0069 / Blocker C — derive the ledger disposition from the ACTUAL
        // admission result returned by `register_typed` rather than from a
        // pre-insertion key-presence check. This is the only way to distinguish
        // a genuine `Installed` from a `DroppedFull` silent no-op when the
        // registry is at the D5 cap.
        let admission = registry.register_typed_with_time(key.clone(), f);
        let disposition = match admission {
            TypedAdmission::Inserted => Some(nmp_core::Disposition::Installed),
            TypedAdmission::Replaced => Some(nmp_core::Disposition::ReplacedPrevious),
            // D5 cap hit: the closure was silently dropped. Record a diagnostic
            // via the ledger with a dedicated disposition so the host can
            // observe the cap-induced drop at composition time (tracing::warn
            // already fired inside `admit_keyed`).
            TypedAdmission::DroppedFull => None,
        };
        if let Some(disp) = disposition {
            composition_ledger.record(
                "typed_snapshot_projection",
                key.clone(),
                key,
                disp,
                None,
            );
        }
    }
}

// Issue #1283 / ADR-0072 — the `refs.event.envelopes` snapshot-projection
// producer. A submodule of `snapshot` (both own snapshot-projection wiring);
// kept here rather than as a `lib.rs` sibling `mod` so the over-cap `lib.rs`
// does not grow (AGENTS.md file-size anti-cheat). See the module doc for the
// one-tick-lag design.
#[path = "embed_sidecar.rs"]
pub(crate) mod embed_sidecar;

// ADR-0070 D7 (#1671 Lane H) — the structural feed-author auto-resolve pairing
// seam (`register_feed_window_source`) + its test introspection accessors. A
// submodule of `snapshot` (both own snapshot-projection wiring); kept off the
// over-cap `lib.rs` AND out of this file so `snapshot.rs` stays under the 500-LOC
// hard ceiling (AGENTS.md file-size anti-cheat).
#[path = "feed_window_source.rs"]
mod feed_window_source;

impl NmpApp {
    /// Register a typed FlatBuffers projection closure for a named projection key.
    ///
    /// The typed sidecar is emitted alongside the existing typed-projection set in
    /// every `SnapshotFrame` (ADR-0072). `f` runs on the actor thread on every
    /// tick — it MUST be non-blocking (D8) and returns `None` when there is no
    /// changed row to emit this tick. Under incremental apply, omission means
    /// retain the last decoded value; unregistering the key emits one `Cleared`
    /// row.
    ///
    /// ADR-0069 — records a truthful composition-ledger disposition:
    /// - [`nmp_core::Disposition::Installed`] — first registration for this key.
    /// - [`nmp_core::Disposition::ReplacedPrevious`] — a closure was already
    ///   registered under this key; the new one wins (last-writer-wins).
    ///
    /// `DroppedLateWiring` is intentionally NEVER recorded here: the registry is
    /// an `Arc<Mutex<SnapshotRegistry>>` that the actor reads on EVERY tick, so a
    /// registration made after start goes live immediately — there is no "too late"
    /// condition for typed snapshot projections.
    pub fn register_typed_snapshot_projection(
        &self,
        key: impl Into<ProjectionRegistrationKey>,
        f: impl Fn() -> Option<nmp_core::TypedProjectionData> + Send + Sync + 'static,
    ) {
        self.register_typed_snapshot_projection_with_time(key, move |_| f());
    }

    /// Register a typed FlatBuffers projection closure that receives the
    /// kernel-authored Unix timestamp for the snapshot tick.
    ///
    /// Most producers should use [`Self::register_typed_snapshot_projection`].
    /// This variant exists for read-only producers that render age/staleness
    /// from actor/kernel time rather than reading the host wall clock.
    pub fn register_typed_snapshot_projection_with_time(
        &self,
        key: impl Into<ProjectionRegistrationKey>,
        f: impl Fn(u64) -> Option<nmp_core::TypedProjectionData> + Send + Sync + 'static,
    ) {
        register_typed_snapshot_projection_on(
            &self.snapshot_projections,
            &self.composition_ledger,
            key,
            f,
        );
    }

    /// Run every registered typed projection closure and collect the emitted
    /// [`TypedProjectionData`](nmp_core::TypedProjectionData) sidecars — the
    /// read counterpart to [`Self::register_typed_snapshot_projection`].
    ///
    /// This is the same vector the actor folds into a snapshot frame's
    /// `typed_projections` sidecar on every tick; exposing it as a `&self`
    /// accessor lets a host (or an app-crate test) introspect what its
    /// registrations actually emit without driving a full snapshot tick. A
    /// closure returning `None` contributes nothing. Pending `Cleared` rows for
    /// removed typed keys are drained exactly once. A poisoned registry mutex
    /// degrades to an empty vector (D6).
    ///
    /// #3080 — this is a production-reachable caller (the NIP-50 search-poll
    /// host, `search.rs`'s `search_snapshot_payload`): a host thread can call
    /// this WHILE a registered snapshot closure opens/closes a read-session.
    /// Runs the same #3079 snapshot-then-release split as the emit loop's
    /// `run_typed_projections` — drain the closure handles under the lock,
    /// release it, then run them via [`nmp_core::__ffi_internal::run_typed_plan`]
    /// — so a closure that re-locks this registry lands cleanly instead of
    /// deadlocking this accessor against itself.
    #[must_use]
    pub fn run_typed_snapshot_projections(&self) -> Vec<nmp_core::TypedProjectionData> {
        let (closures, cleared) = match self.snapshot_projections.lock() {
            Ok(mut registry) => {
                // ADR-0070 D7 (#1671 Lane H) — OUT-OF-BAND introspection path
                // (tests/hosts reading the sidecar without a full `make_update`).
                // Bump the per-tick rev so a `FeedWindowSource` memo re-materializes
                // per ad-hoc call (reflecting a `load_older` grow between calls).
                // The production tick path is a different method, so no double-bump.
                registry.bump_frame_tick_rev();
                registry.drain_typed_run_plan()
            }
            Err(_) => return Vec::new(),
        };
        // `now_secs = 0`: matches the prior `registry.run_typed()` body
        // (`run_typed_at(0)`) — this out-of-band path has no kernel clock to
        // stamp with, same as before.
        nmp_core::__ffi_internal::run_typed_plan(&closures, 0, cleared)
    }

    /// Return the set of typed projection keys currently registered — without
    /// calling any closures. Use this in coverage-gate tests where you need to
    /// verify that a projection key was registered regardless of whether it has
    /// data to emit for the current state.
    #[must_use]
    pub fn registered_typed_projection_keys(&self) -> Vec<String> {
        self.snapshot_projections
            .lock()
            .map(|registry| registry.registered_typed_keys().map(String::from).collect())
            .unwrap_or_default()
    }
}
