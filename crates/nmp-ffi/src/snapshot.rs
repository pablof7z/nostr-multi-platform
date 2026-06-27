//! FFI snapshot-projection registration entry point.
//!
//! Provides the typed (FlatBuffers) registration seam ([`NmpApp::register_typed_snapshot_projection`])
//! and C-ABI declaration surfaces for consumed projections and incremental-apply.
//! The generic (`serde_json::Value`) lane has been removed; all projections use
//! the typed FlatBuffers sidecar (ADR-0037).

use std::ffi::{c_char, CStr};

use super::{app_ref, NmpApp};

// Issue #1283 / ADR-0034 — the `claimed_event_embeds` snapshot-projection
// producer. A submodule of `snapshot` (both own snapshot-projection wiring);
// kept here rather than as a `lib.rs` sibling `mod` so the over-cap `lib.rs`
// does not grow (AGENTS.md file-size anti-cheat). See the module doc for the
// one-tick-lag design.
#[path = "embed_sidecar.rs"]
pub(crate) mod embed_sidecar;

// ADR-0063 D7 (#1671 Lane H) — the structural feed-author auto-resolve pairing
// seam (`register_feed_render_source`) + its test introspection accessors. A
// submodule of `snapshot` (both own snapshot-projection wiring); kept off the
// over-cap `lib.rs` AND out of this file so `snapshot.rs` stays under the 500-LOC
// hard ceiling (AGENTS.md file-size anti-cheat).
#[path = "feed_render_source.rs"]
mod feed_render_source;

impl NmpApp {
    /// Register a typed FlatBuffers projection closure for a named projection key.
    ///
    /// The typed sidecar is emitted alongside the existing typed-projection set in
    /// every `SnapshotFrame` (ADR-0037). `f` runs on the actor thread on every
    /// tick — it MUST be non-blocking (D8) and returns `None` when there is no
    /// changed row to emit this tick. Under incremental apply, omission means
    /// retain the last decoded value; unregistering the key emits one `Cleared`
    /// row.
    ///
    /// ADR-0049 — records a truthful composition-ledger disposition:
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
        key: impl Into<String>,
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
        key: impl Into<String>,
        f: impl Fn(u64) -> Option<nmp_core::TypedProjectionData> + Send + Sync + 'static,
    ) {
        use nmp_core::__ffi_internal::TypedAdmission;
        let key = key.into();
        if let Ok(mut registry) = self.snapshot_projections.lock() {
            // ADR-0049 / Blocker C — derive the ledger disposition from the
            // ACTUAL admission result returned by `register_typed` rather than
            // from a pre-insertion key-presence check. This is the only way to
            // distinguish a genuine `Installed` from a `DroppedFull` silent
            // no-op when the registry is at the D5 cap.
            let admission = registry.register_typed_with_time(key.clone(), f);
            let disposition = match admission {
                TypedAdmission::Inserted => Some(nmp_core::Disposition::Installed),
                TypedAdmission::Replaced => Some(nmp_core::Disposition::ReplacedPrevious),
                // D5 cap hit: the closure was silently dropped. Record a
                // diagnostic via the ledger with a dedicated disposition so
                // the host can observe the cap-induced drop at composition
                // time (tracing::warn already fired inside `admit_keyed`).
                TypedAdmission::DroppedFull => None,
            };
            if let Some(disp) = disposition {
                self.composition_ledger.record(
                    "typed_snapshot_projection",
                    key.clone(),
                    key,
                    disp,
                    None,
                );
            }
        }
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
    #[must_use]
    pub fn run_typed_snapshot_projections(&self) -> Vec<nmp_core::TypedProjectionData> {
        self.snapshot_projections
            .lock()
            .map(|mut registry| {
                // ADR-0063 D7 (#1671 Lane H) — OUT-OF-BAND introspection path
                // (tests/hosts reading the sidecar without a full `make_update`).
                // Bump the per-tick rev so a `FeedRenderSource` memo re-materializes
                // per ad-hoc call (reflecting a `load_older` grow between calls).
                // The production tick path is a different method, so no double-bump.
                registry.bump_frame_tick_rev();
                registry.run_typed()
            })
            .unwrap_or_default()
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

    /// Register a per-tick observer — a no-result callback fired once on every
    /// snapshot tick. The generic, projection-free counterpart to
    /// [`Self::register_snapshot_projection`]: it contributes no snapshot key,
    /// it is a pure per-tick side-effect seam (e.g. an active-account
    /// subscription reconciler that enqueues `EnsureInterest` / `DropInterestOwner`
    /// each tick).
    ///
    /// Like `register_snapshot_projection`, this takes `&self` (the mutation is
    /// a lock-and-push behind the shared registry slot) and is intended as a
    /// host-init call. `f` runs on the actor thread inside the tick — it MUST be
    /// non-blocking (D8). A poisoned registry mutex is a silent no-op (D6).
    pub fn register_snapshot_tick_observer(&self, f: impl Fn() + Send + Sync + 'static) {
        if let Ok(mut registry) = self.snapshot_projections.lock() {
            registry.register_tick_observer(f);
        }
    }

    /// Register or replace a keyed per-tick observer.
    ///
    /// Use this for lifecycle-bound protocol observers that should have at
    /// most one live callback per key, such as account-scoped reconcilers.
    pub fn replace_snapshot_tick_observer(
        &self,
        key: impl Into<String>,
        f: impl Fn() + Send + Sync + 'static,
    ) {
        if let Ok(mut registry) = self.snapshot_projections.lock() {
            registry.replace_tick_observer(key, f);
        }
    }

    /// Remove a keyed per-tick observer. Missing keys are a D6 no-op.
    pub fn remove_snapshot_tick_observer(&self, key: &str) {
        if let Ok(mut registry) = self.snapshot_projections.lock() {
            registry.remove_tick_observer(key);
        }
    }
}

/// ADR-0055 Rung 3 — declare that this host runtime owns the NMP cache-merge
/// layer (D3-3) and is ready to receive frames with `Unchanged` projections
/// omitted.
///
/// Must be called before `nmp_app_start`. After this call the kernel guarantees
/// the next `make_update` frame is a full baseline (all live Tier-2 projections
/// emitted as `Changed`). Until this is called the kernel emits full rows on
/// every tick (safe for non-advertising hosts). Idempotent — subsequent calls
/// before start return 0 without re-setting the latch.
///
/// S1b finding 5 (issue #1390): returns an `i32` return-code instead of
/// `void` so the caller can detect a post-start or registry-error condition
/// in all build configurations (replacing the prior `debug_assert!` which was
/// silent in release):
///
/// - `0`  = ok (or idempotent repeat call before start)
/// - `1`  = `AlreadyStarted` — called after `nmp_app_start`
/// - `2`  = `RegistryUnavailable` — registry mutex poisoned
/// - `-1` = null `app` pointer (D6: defined return code, not a crash)
///
/// # Safety
/// `app` must be a valid pointer from [`super::nmp_app_new`] (or null).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_declare_incremental_apply(app: *mut NmpApp) -> i32 {
    use nmp_core::substrate::IncrementalApplyError;
    let Some(app) = app_ref(app) else {
        return -1;
    };
    match app.declare_incremental_apply() {
        Ok(()) => 0,
        Err(IncrementalApplyError::AlreadyStarted) => 1,
        Err(IncrementalApplyError::RegistryUnavailable) => 2,
    }
}

/// ADR-0053 — declare the static set of Tier-2 built-in projection keys this
/// host consumes (the output-side sibling of relay interest installs).
///
/// `keys` is a host-owned array of `len` NUL-terminated UTF-8 C strings (the
/// union of every projection key any of the app's screens reads, known at app
/// build time). The kernel then serializes a kernel-owned built-in into each
/// snapshot only if its key is declared. An empty / zero-length declaration
/// leaves the kernel emitting every built-in (no narrowing); a non-empty
/// declaration narrows the built-ins to the declared members, skipping the
/// producer work (notably the `relay_diagnostics` roll-up) for everything else.
///
/// Additive — multiple calls union. Intended as a host-init call, before
/// `nmp_app_start`. Individual null / non-UTF-8 entries are skipped; a null
/// `app` or null `keys` is a silent no-op (D6: a bad registration argument never
/// crashes the host).
///
/// # Safety
/// `app` must be a valid pointer from [`super::nmp_app_new`] (or null).
/// `keys`, when non-null, must point to `len` valid `*const c_char`, each a
/// valid NUL-terminated C string (or null) live for the duration of this call.
/// The pointers are read and copied immediately; the host retains ownership.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_declare_consumed_projections(
    app: *mut NmpApp,
    keys: *const *const c_char,
    len: usize,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    if keys.is_null() || len == 0 {
        return;
    }
    let mut declared: Vec<String> = Vec::with_capacity(len);
    for i in 0..len {
        // SAFETY: per the contract, `keys` points to `len` valid `*const c_char`.
        let entry = unsafe { *keys.add(i) };
        if entry.is_null() {
            continue;
        }
        // SAFETY: a non-null entry is a valid NUL-terminated C string for the
        // duration of this read; the bytes are copied immediately.
        let s = unsafe { CStr::from_ptr(entry) }
            .to_string_lossy()
            .into_owned();
        if !s.is_empty() {
            declared.push(s);
        }
    }
    app.declare_consumed_projections(declared);
}

/// ADR-0053 / Workstream-E4 — declare the explicit "I consume every Tier-2
/// built-in projection" intent (`DeclaredProjections::All`).
///
/// This is the ONE non-footgun way to receive the full built-in set: a host
/// that genuinely reads everything (a full client like chirp-tui / chirp-desktop,
/// or the Chirp shells) calls this instead of leaving the consumption intent
/// undeclared (which `nmp_app_start` treats as a loud forgotten-wiring bug, not
/// a silent firehose).
///
/// Idempotent; intended as a host-init call before `nmp_app_start`. A null `app`
/// is a silent no-op (D6).
///
/// # Safety
/// `app` must be a valid pointer from [`super::nmp_app_new`] (or null).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_consume_all_builtin_projections(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    app.consume_all_builtin_projections();
}

#[cfg(test)]
#[path = "snapshot/tests.rs"]
mod tests;
