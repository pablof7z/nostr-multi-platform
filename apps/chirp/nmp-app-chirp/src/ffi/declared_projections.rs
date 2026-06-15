//! ADR-0053 / Workstream-E4 — Chirp's projection-consumption intent.
//!
//! Both Chirp shells (iOS SwiftUI, Android Compose) — and the chirp-tui /
//! chirp-desktop Rust shells — are **full clients**: they consume every Tier-2
//! kernel-owned built-in projection. So Chirp's intent is the explicit
//! `consume_all_builtin_projections` (`DeclaredProjections::All`), not a
//! hand-maintained key list.
//!
//! This retires the former `CHIRP_CONSUMED_BUILTIN_PROJECTIONS` array (a
//! hand-kept copy of all 18 `KERNEL_BUILTIN_PROJECTION_KEYS`) and its
//! bidirectional drift tests: with `consume_all` there is no list to drift, and
//! the kernel built-in key set is now codegen-derived from the single
//! `nmp-codegen` projection registry (see
//! `crates/nmp-codegen/src/swift_projections_registry.rs::kernel_builtin_projection_keys`).
//!
//! Tier-1 host/protocol projections (`nmp.feed.*`, `nmp.nip29.*`, `nmp.nip17.*`,
//! `nmp.nip57.*`, `nmp.marmot.*`, `wallet`, `bunker_handshake`,
//! `nip46_onboarding`, `signer_state`) are NOT affected: they self-gate by
//! registration (registration IS the declaration).

use nmp_ffi::NmpApp;

/// ADR-0053 / Workstream-E4 — declare Chirp's projection-consumption intent on
/// `app`: the explicit "consume every Tier-2 built-in" (`consume_all`). Chirp's
/// four shells (iOS, Android, tui, desktop) all read the full built-in set, so
/// this is the deliberate, greppable `All` choice — never the silent
/// undeclared footgun. Idempotent; call once at app construction, before
/// `nmp_app_start`. A null `app` is a silent no-op (D6).
///
/// # Safety
/// `app` must be a valid pointer from `nmp_app_new()` (or null).
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_declare_consumed_projections(app: *mut NmpApp) {
    // Chirp is a full client — it reads every kernel built-in. Reuse the generic
    // FFI seam so a single code path expresses "everything" (no Chirp-local key
    // list to drift from the kernel built-in set).
    nmp_ffi::nmp_app_consume_all_builtin_projections(app);
}
