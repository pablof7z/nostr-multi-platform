//! FFI snapshot-projection registration entry point.
//!
//! Provides the typed (FlatBuffers) registration seam ([`NmpApp::register_typed_snapshot_projection`])
//! and C-ABI declaration surfaces for consumed projections and incremental-apply.
//! The generic (`serde_json::Value`) lane has been removed; all projections use
//! the typed FlatBuffers sidecar (ADR-0037).

use std::ffi::{c_char, CStr};

use super::{app_ref, NmpApp};

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
