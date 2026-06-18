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
        use nmp_core::__ffi_internal::TypedAdmission;
        let key = key.into();
        if let Ok(mut registry) = self.snapshot_projections.lock() {
            // ADR-0049 / Blocker C — derive the ledger disposition from the
            // ACTUAL admission result returned by `register_typed` rather than
            // from a pre-insertion key-presence check. This is the only way to
            // distinguish a genuine `Installed` from a `DroppedFull` silent
            // no-op when the registry is at the D5 cap.
            let admission = registry.register_typed(key.clone(), f);
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
            .map(|mut registry| registry.run_typed())
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
    /// subscription reconciler that enqueues `PushInterest` / `WithdrawInterest`
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
/// host consumes (the output-side sibling of relay `push_interest`).
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
mod tests {
    use super::super::{nmp_app_free, nmp_app_new};
    use super::*;
    use nmp_core::substrate::SnapshotProjectionRegistrar;
    use nmp_core::TypedProjectionData;
    use std::ffi::CString;

    /// ADR-0053 — the C-ABI declaration seam unions keys into the registry's
    /// declared set (read back through the shared `NmpApp` registry clone).
    #[test]
    fn declare_consumed_projections_unions_keys_into_registry() {
        let app = nmp_app_new();
        let k1 = CString::new("profile").unwrap();
        let k2 = CString::new("accounts").unwrap();
        let arr: [*const c_char; 2] = [k1.as_ptr(), k2.as_ptr()];
        nmp_app_declare_consumed_projections(app, arr.as_ptr(), arr.len());

        // SAFETY: `nmp_app_new` never returns null.
        let app_ref = unsafe { &*app };
        let registry = app_ref.snapshot_projections.lock().expect("registry lock");
        let declared = registry.declared_projections();
        assert!(declared.is_narrowing(), "a non-empty declaration narrows");
        assert!(declared.permits("profile"));
        assert!(declared.permits("accounts"));
        assert!(
            !declared.permits("relay_diagnostics"),
            "undeclared key is gated out once a non-empty set is declared"
        );
        drop(registry);
        nmp_app_free(app);
    }

    /// A null `app` / null `keys` / zero `len` declaration is a silent no-op (D6).
    #[test]
    fn declare_consumed_projections_bad_args_are_noops() {
        // Null app — must not crash.
        let k = CString::new("profile").unwrap();
        let arr: [*const c_char; 1] = [k.as_ptr()];
        nmp_app_declare_consumed_projections(std::ptr::null_mut(), arr.as_ptr(), arr.len());

        let app = nmp_app_new();
        // Null keys pointer — no-op, set stays empty (no narrowing).
        nmp_app_declare_consumed_projections(app, std::ptr::null(), 3);
        // Zero len — no-op.
        nmp_app_declare_consumed_projections(app, arr.as_ptr(), 0);
        // SAFETY: `nmp_app_new` never returns null.
        let app_ref = unsafe { &*app };
        let registry = app_ref.snapshot_projections.lock().expect("registry lock");
        assert!(
            !registry.declared_projections().is_narrowing(),
            "bad-arg declarations leave the set empty (no narrowing)"
        );
        drop(registry);
        nmp_app_free(app);
    }

    /// ADR-0037 — the typed-projection registration seam is reachable through
    /// the narrow `SnapshotProjectionRegistrar` **trait** (was concrete-only on
    /// `NmpApp`), so a reusable protocol/feed crate that wires through
    /// `register_runtime(app: &mut impl SnapshotProjectionRegistrar)` can
    /// register a typed FlatBuffers projection. This mirrors
    /// `registered_typed_projection_surfaces_through_run_typed`
    /// (`nmp-core/src/kernel/snapshot_registry_tests.rs`) but drives the
    /// registration through `&impl AppHost` — the exact path protocol crates
    /// use — and asserts the typed projection surfaces in the typed sidecar.
    #[test]
    fn typed_projection_registered_through_trait_surfaces_in_sidecar() {
        // Register through `&impl SnapshotProjectionRegistrar`, NOT the inherent
        // `NmpApp` method — this is the seam protocol crates reach via
        // `register_runtime`.
        fn register_via_trait(host: &impl SnapshotProjectionRegistrar) {
            host.register_typed_snapshot_projection("nmp.feed.home", || {
                Some(TypedProjectionData {
                    key: "nmp.feed.home".to_string(),
                    schema_id: "nmp.nip01.timeline".to_string(),
                    schema_version: 1,
                    file_identifier: "NFTS".to_string(),
                    payload: vec![0xde, 0xad, 0xbe, 0xef],
                    ..Default::default()
                })
            });
        }

        let app = nmp_app_new();
        // SAFETY: `nmp_app_new` never returns null.
        let app_ref = unsafe { &*app };
        register_via_trait(app_ref);

        let typed = app_ref.run_typed_snapshot_projections_for_test();
        let entry = typed.iter().find(|d| d.key == "nmp.feed.home").expect(
            "a typed projection registered through the SnapshotProjectionRegistrar trait must \
             surface in run_typed",
        );
        assert_eq!(entry.schema_id, "nmp.nip01.timeline");
        assert_eq!(entry.schema_version, 1);
        assert_eq!(entry.file_identifier, "NFTS");
        assert_eq!(entry.payload, vec![0xde, 0xad, 0xbe, 0xef]);
        nmp_app_free(app);
    }

    /// ADR-0049 / Blocker C — `register_typed_snapshot_projection` records a
    /// truthful composition-ledger disposition:
    /// - First registration for a key → `Installed`.
    /// - Second registration for the same key → `ReplacedPrevious`.
    /// - Over-cap new key → NO ledger entry (silent drop, not a false `Installed`).
    /// - `DroppedLateWiring` is never recorded (the typed registry is live at all
    ///   times — there is no "post-start drop" for it).
    #[test]
    fn typed_projection_records_composition_ledger_disposition() {
        let app = nmp_app_new();
        // SAFETY: `nmp_app_new` never returns null.
        let app_ref = unsafe { &*app };

        // First registration: Installed.
        app_ref.register_typed_snapshot_projection("nmp.feed.home", || None);
        let ledger_json = app_ref.composition_ledger().to_json();
        let records = ledger_json["records"]
            .as_array()
            .expect("composition ledger must have a records array");
        let first = records
            .iter()
            .find(|r| r["key"] == "nmp.feed.home")
            .expect("ledger must contain an entry for nmp.feed.home after first registration");
        assert_eq!(
            first["seam"], "typed_snapshot_projection",
            "seam must be typed_snapshot_projection"
        );
        // serde derives serialize `Disposition::Installed` as `"Installed"`.
        assert_eq!(
            first["disposition"], "Installed",
            "first registration must record Installed"
        );
        assert!(
            first.get("replaced").is_none() || first["replaced"].is_null(),
            "Installed disposition must not carry a replaced field"
        );

        // Second registration for the same key: ReplacedPrevious.
        app_ref.register_typed_snapshot_projection("nmp.feed.home", || None);
        let ledger_json2 = app_ref.composition_ledger().to_json();
        let records2 = ledger_json2["records"]
            .as_array()
            .expect("records array present");
        let home_records: Vec<_> = records2
            .iter()
            .filter(|r| r["key"] == "nmp.feed.home")
            .collect();
        assert_eq!(
            home_records.len(),
            2,
            "two registrations must produce two ledger entries"
        );
        let second = &home_records[1];
        assert_eq!(
            second["disposition"], "ReplacedPrevious",
            "second registration for the same key must record ReplacedPrevious"
        );

        // Distinct key: separate Installed entry.
        app_ref.register_typed_snapshot_projection("nmp.nip17.dm_inbox", || None);
        let ledger_json3 = app_ref.composition_ledger().to_json();
        let records3 = ledger_json3["records"]
            .as_array()
            .expect("records array present");
        let dm = records3
            .iter()
            .find(|r| r["key"] == "nmp.nip17.dm_inbox")
            .expect("dm_inbox entry must be in ledger");
        assert_eq!(
            dm["disposition"], "Installed",
            "distinct key must record Installed, not ReplacedPrevious"
        );

        nmp_app_free(app);
    }

    /// Blocker C — over-cap new key must NOT produce a false `Installed` ledger
    /// entry. The D5 ceiling is 64; when the 65th distinct key is registered
    /// the registry drops it silently (`TypedAdmission::DroppedFull`) and the
    /// caller must NOT record `Installed` for the ghost key.
    #[test]
    fn over_cap_typed_projection_does_not_record_installed() {
        use nmp_core::__ffi_internal::MAX_SNAPSHOT_PROJECTIONS;

        let app = nmp_app_new();
        // SAFETY: `nmp_app_new` never returns null.
        let app_ref = unsafe { &*app };

        // Fill the registry to the exact cap with distinct keys.
        for i in 0..MAX_SNAPSHOT_PROJECTIONS {
            app_ref.register_typed_snapshot_projection(format!("nmp.test.key{i}"), || None);
        }

        // Verify the cap keys are in the ledger.
        let ledger_before = app_ref.composition_ledger().to_json();
        let records_before = ledger_before["records"].as_array().expect("records array");
        let count_before = records_before.len();
        assert_eq!(
            count_before,
            MAX_SNAPSHOT_PROJECTIONS,
            "ledger must have exactly {MAX_SNAPSHOT_PROJECTIONS} entries after filling to cap"
        );

        // Now attempt to register one MORE distinct key — should be dropped.
        let over_cap_key = "nmp.test.over_cap_key";
        app_ref.register_typed_snapshot_projection(over_cap_key, || None);

        // The over-cap key must NOT appear in the ledger.
        let ledger_after = app_ref.composition_ledger().to_json();
        let records_after = ledger_after["records"].as_array().expect("records array");
        let has_over_cap_entry = records_after.iter().any(|r| r["key"] == over_cap_key);
        assert!(
            !has_over_cap_entry,
            "an over-cap registration must NOT produce a ledger entry (no false Installed)"
        );
        assert_eq!(
            records_after.len(),
            MAX_SNAPSHOT_PROJECTIONS,
            "ledger size must remain {MAX_SNAPSHOT_PROJECTIONS} after over-cap attempt"
        );

        // Replacing an existing key at cap IS still allowed and MUST record ReplacedPrevious.
        let first_key = "nmp.test.key0";
        app_ref.register_typed_snapshot_projection(first_key, || None);
        let ledger_after_replace = app_ref.composition_ledger().to_json();
        let records_after_replace = ledger_after_replace["records"].as_array().expect("records array");
        let replaced_entries: Vec<_> = records_after_replace
            .iter()
            .filter(|r| r["key"] == first_key)
            .collect();
        assert_eq!(
            replaced_entries.len(),
            2,
            "replacing an existing key at cap must produce two ledger entries"
        );
        assert_eq!(
            replaced_entries[1]["disposition"], "ReplacedPrevious",
            "second registration for an existing key at cap must record ReplacedPrevious"
        );

        nmp_app_free(app);
    }
}
