//! FFI snapshot-projection registration entry point.
//!
//! [`nmp_app_register_snapshot_projection`] is the output-side counterpart to
//! the action-registry seam (`NmpApp::register_action::<M>()`). Where the
//! action seam lets a host *dispatch* a custom namespace, this seam lets a
//! host *project* a custom namespace into every snapshot.
//!
//! # The seam
//!
//! `KernelSnapshot` is a sealed social wire schema. A host registers a
//! **snapshot projector** — a C callback invoked on every snapshot tick whose
//! returned JSON string is appended to `KernelSnapshot::projections` under a
//! host-chosen key. A marketplace app registers `"market.listings"`, a todo
//! app `"todo.items"` — each gets its own namespace WITHOUT editing
//! `nmp-core`'s typed social fields.
//!
//! # Doctrine
//!
//! * **D6** — a null `app`, a null/empty/invalid `key`, or a null `projector`
//!   is a silent no-op. A bad registration argument never crashes the host.
//! * **D8** — the projector callback runs on the actor thread inside the
//!   snapshot tick. It MUST be cheap and non-blocking; a blocking projector
//!   stalls every subsequent snapshot.

use std::ffi::{c_char, CStr};

use super::{app_ref, c_string_argument, NmpApp};

/// Host-supplied snapshot projector callback.
///
/// Called on every snapshot tick. Returns a NUL-terminated JSON C string
/// contributed to the host's projection key, or `NULL` to contribute an empty
/// JSON object. The returned pointer is read immediately and copied into an
/// owned Rust value; the host owns its lifetime and may free or reuse it
/// after the callback returns.
///
/// A non-JSON / un-parseable return is treated as JSON `null` (D6: a bad
/// projector return is data, never a panic).
pub type NmpSnapshotProjector = unsafe extern "C" fn() -> *const c_char;

impl NmpApp {
    /// Register a typed FlatBuffers projection closure for a named projection key.
    ///
    /// The typed sidecar is emitted alongside the existing generic `Value` tree in
    /// every `SnapshotFrame` (ADR-0037). `f` runs on the actor thread on every
    /// tick — it MUST be non-blocking (D8) and returns `None` when there is
    /// nothing to emit this tick.
    pub fn register_typed_snapshot_projection(
        &self,
        key: impl Into<String>,
        f: impl Fn() -> Option<nmp_core::TypedProjectionData> + Send + Sync + 'static,
    ) {
        if let Ok(mut registry) = self.snapshot_projections.lock() {
            registry.register_typed(key, f);
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
    /// closure returning `None` contributes nothing. A poisoned registry mutex
    /// degrades to an empty vector (D6).
    #[must_use]
    pub fn run_typed_snapshot_projections(&self) -> Vec<nmp_core::TypedProjectionData> {
        self.snapshot_projections
            .lock()
            .map(|registry| registry.run_typed())
            .unwrap_or_default()
    }

    /// Run every registered **generic** (`Value`) projection closure and collect
    /// the `key → JSON` map — the JSON-side counterpart to
    /// [`Self::run_typed_snapshot_projections`].
    ///
    /// This is the same map the actor folds into a snapshot frame's generic
    /// `payload:Value.projections` subtree on every tick. Exposing it as a
    /// `&self` accessor lets the `payload:Value` → `typed_projections` migration
    /// **prove producer-completeness**: any key present here but absent from
    /// `run_typed_snapshot_projections()` is a generic projection whose typed
    /// sidecar a consumer's `typed<K> ?? snapshot?.<k>` fallback would silently
    /// hide — exactly the key that breaks if the JSON fallback is removed. A
    /// closure returning `null` contributes nothing (same emit condition as its
    /// paired typed closure). A poisoned registry mutex degrades to an empty
    /// map (D6).
    #[must_use]
    pub fn run_snapshot_projections(&self) -> std::collections::HashMap<String, serde_json::Value> {
        self.snapshot_projections
            .lock()
            .map(|registry| registry.run())
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

/// Register a host-supplied snapshot projector for `key` — the host-extensible
/// snapshot-output seam.
///
/// This is the C-ABI counterpart to [`NmpApp::register_snapshot_projection`]:
/// a host wires a snapshot namespace into the kernel **without editing
/// `nmp-core`**. The bridge closure invokes `projector`, parses its returned
/// JSON string, and the kernel appends the result under `key` in
/// `KernelSnapshot::projections` on every tick.
///
/// The projection registry lives behind a shared `Arc<Mutex<…>>` slot bound
/// onto the actor-thread-owned kernel; this call only takes `&NmpApp` (the
/// mutation is a lock-and-push), so it is safe to call concurrently with a
/// running actor. It is still intended as a host-init call.
///
/// A null `app`, a null/empty/invalid `key`, or a null `projector` is a
/// silent no-op (D6: a bad registration argument never crashes the host).
///
/// # Safety
/// `app` must be a valid pointer from [`super::nmp_app_new`] (or null).
/// `key` must be a valid UTF-8 NUL-terminated C string (or null).
/// `projector`, when `Some`, must be a valid function pointer for the
/// remaining lifetime of `app` — the registry retains it.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_register_snapshot_projection(
    app: *mut NmpApp,
    key: *const c_char,
    projector: Option<NmpSnapshotProjector>,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(key) = c_string_argument(key) else {
        return;
    };
    let Some(projector) = projector else {
        return;
    };
    app.register_snapshot_projection(key, move || {
        // SAFETY: `projector` is a valid function pointer per this symbol's
        // safety contract.
        let ptr = unsafe { projector() };
        if ptr.is_null() {
            // A NULL return contributes an empty JSON object.
            return serde_json::Value::Object(serde_json::Map::new());
        }
        // SAFETY: a non-null return is, per the callback contract, a valid
        // NUL-terminated C string live for the duration of this read. The
        // bytes are copied immediately; the host retains ownership.
        let json = unsafe { CStr::from_ptr(ptr) }.to_string_lossy();
        // D6: an un-parseable projector return collapses to JSON `null`
        // rather than panicking across the C ABI boundary.
        serde_json::from_str(&json).unwrap_or(serde_json::Value::Null)
    });
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

#[cfg(test)]
mod tests {
    use super::super::{nmp_app_free, nmp_app_new};
    use super::*;
    use nmp_core::substrate::AppHost;
    use nmp_core::TypedProjectionData;
    use std::ffi::CString;

    /// A registered C projector contributes a parsed JSON value under its key.
    /// Uses a `static` C string so the returned pointer outlives the call —
    /// the real ABI contract only requires it live for the read.
    extern "C" fn counter_projector() -> *const c_char {
        // `c"…"` literal: a `'static` NUL-terminated C string, valid for the
        // whole program — satisfies the projector-return lifetime contract.
        c"{\"count\":42}".as_ptr()
    }

    /// A projector returning NULL contributes an empty JSON object.
    extern "C" fn null_projector() -> *const c_char {
        std::ptr::null()
    }

    #[test]
    fn register_snapshot_projection_runs_c_projector() {
        let app = nmp_app_new();
        let key = CString::new("test.counter").unwrap();
        nmp_app_register_snapshot_projection(app, key.as_ptr(), Some(counter_projector));
        // SAFETY: `nmp_app_new` never returns null.
        let app_ref = unsafe { &*app };
        let projections = app_ref.run_snapshot_projections_for_test();
        assert_eq!(
            projections.get("test.counter").and_then(|v| v.get("count")),
            Some(&serde_json::json!(42)),
            "C projector return must be parsed under its key"
        );
        nmp_app_free(app);
    }

    #[test]
    fn null_projector_return_contributes_empty_object() {
        let app = nmp_app_new();
        let key = CString::new("test.empty").unwrap();
        nmp_app_register_snapshot_projection(app, key.as_ptr(), Some(null_projector));
        // SAFETY: `nmp_app_new` never returns null.
        let app_ref = unsafe { &*app };
        let projections = app_ref.run_snapshot_projections_for_test();
        assert_eq!(
            projections.get("test.empty"),
            Some(&serde_json::json!({})),
            "a NULL projector return is an empty JSON object"
        );
        nmp_app_free(app);
    }

    #[test]
    fn null_app_is_silent_noop() {
        let key = CString::new("test.counter").unwrap();
        // Must not panic / crash — D6.
        nmp_app_register_snapshot_projection(
            std::ptr::null_mut(),
            key.as_ptr(),
            Some(counter_projector),
        );
    }

    #[test]
    fn null_key_is_silent_noop() {
        let app = nmp_app_new();
        nmp_app_register_snapshot_projection(app, std::ptr::null(), Some(counter_projector));
        // SAFETY: `nmp_app_new` never returns null.
        let app_ref = unsafe { &*app };
        // A null key must register nothing — the registry contains only the
        // built-in `"wallet"` projection (`feature = "wallet"`), never the
        // test's `test.counter` key.
        assert!(
            !app_ref
                .run_snapshot_projections_for_test()
                .contains_key("test.counter"),
            "a null key must register nothing"
        );
        nmp_app_free(app);
    }

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

    #[test]
    fn null_projector_is_silent_noop() {
        let app = nmp_app_new();
        let key = CString::new("test.counter").unwrap();
        nmp_app_register_snapshot_projection(app, key.as_ptr(), None);
        // SAFETY: `nmp_app_new` never returns null.
        let app_ref = unsafe { &*app };
        // A null projector must register nothing — the registry never gains
        // the test's `test.counter` key (the built-in `"wallet"` projection
        // under `feature = "wallet"` may still be present).
        assert!(
            !app_ref
                .run_snapshot_projections_for_test()
                .contains_key("test.counter"),
            "a null projector must register nothing"
        );
        nmp_app_free(app);
    }

    /// ADR-0037 — the typed-projection registration seam is reachable through
    /// the `AppHost` **trait** (was concrete-only on `NmpApp`), so a reusable
    /// protocol/feed crate that wires through `register_runtime(app: &impl
    /// AppHost)` can register a typed FlatBuffers projection. This mirrors
    /// `registered_typed_projection_surfaces_through_run_typed`
    /// (`nmp-core/src/kernel/snapshot_registry_tests.rs`) but drives the
    /// registration through `&impl AppHost` — the exact path protocol crates
    /// use — and asserts the typed projection surfaces in the typed sidecar.
    #[test]
    fn typed_projection_registered_through_trait_surfaces_in_sidecar() {
        // Register through `&impl AppHost`, NOT the inherent `NmpApp` method —
        // this is the seam protocol crates reach via `register_runtime`.
        fn register_via_trait(host: &impl AppHost) {
            host.register_typed_snapshot_projection("nmp.feed.home", || {
                Some(TypedProjectionData {
                    key: "nmp.feed.home".to_string(),
                    schema_id: "nmp.nip01.timeline".to_string(),
                    schema_version: 1,
                    file_identifier: "NFTS".to_string(),
                    payload: vec![0xde, 0xad, 0xbe, 0xef],
                })
            });
        }

        let app = nmp_app_new();
        // SAFETY: `nmp_app_new` never returns null.
        let app_ref = unsafe { &*app };
        register_via_trait(app_ref);

        let typed = app_ref.run_typed_snapshot_projections_for_test();
        let entry = typed.iter().find(|d| d.key == "nmp.feed.home").expect(
            "a typed projection registered through the AppHost trait must surface in run_typed",
        );
        assert_eq!(entry.schema_id, "nmp.nip01.timeline");
        assert_eq!(entry.schema_version, 1);
        assert_eq!(entry.file_identifier, "NFTS");
        assert_eq!(entry.payload, vec![0xde, 0xad, 0xbe, 0xef]);
        nmp_app_free(app);
    }
}
