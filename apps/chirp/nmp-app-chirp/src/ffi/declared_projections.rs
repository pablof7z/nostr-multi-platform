//! ADR-0053 — Chirp's host-declared Tier-2 built-in projection consumption set.
//!
//! Both Chirp shells (iOS SwiftUI, Android Compose) consume the same Tier-2
//! kernel-owned built-ins. Rather than have each shell ship a hand-kept key
//! array across the FFI boundary, the set lives here once — the Chirp app crate —
//! and a single C-ABI call declares it. Both shells call
//! [`nmp_app_chirp_declare_consumed_projections`] at app construction.
//!
//! Tier-1 host/protocol projections (`nmp.feed.*`, `nmp.nip29.*`, `nmp.nip17.*`,
//! `nmp.nip57.*`, `nmp.marmot.*`, `wallet`, `bunker_handshake`,
//! `nip46_onboarding`, `signer_state`) are NOT listed here: they self-gate by
//! registration (registration IS the declaration) and the dynamic feeds gate by
//! their open/close lifecycle. Only the un-gated Tier-2 built-ins
//! ([`nmp_core::KERNEL_BUILTIN_PROJECTION_KEYS`]) need a declared-set entry.
//!
//! The `every_chirp_declared_key_is_a_kernel_builtin` test pins this list against
//! `KERNEL_BUILTIN_PROJECTION_KEYS` so a producer-side built-in rename cannot
//! silently drift the Chirp declaration.

use std::ffi::c_char;

use nmp_ffi::NmpApp;

/// The Tier-2 kernel-owned built-in projection keys both Chirp shells consume.
///
/// This is the union of the iOS and Android consumed sets, restricted to the
/// keys in [`nmp_core::KERNEL_BUILTIN_PROJECTION_KEYS`] (the kernel-owned
/// built-ins). Every member must be a kernel built-in (pinned by test).
pub const CHIRP_CONSUMED_BUILTIN_PROJECTIONS: &[&str] = &[
    // Identity + profile.
    "accounts",
    "active_account",
    "profile",
    // Profile/event reference resolution (component claim path).
    "resolved_profiles",
    "claimed_profiles",
    "claimed_events",
    // Mention-author map (Android decodes it; harmless for iOS to receive).
    "mention_profiles",
    // Publish / relay-settings cluster.
    "publish_queue",
    "publish_outbox",
    "outbox_summary",
    "configured_relays",
    "relay_role_options",
    "settings_hub",
    // Action lifecycle cluster (drain-on-emit; declared so they surface on
    // settle ticks).
    "action_results",
    "signed_events",
    "action_stages",
    "action_lifecycle",
    // Diagnostics screen. Chirp ships a relay-diagnostics view, so it declares
    // this; non-diagnostics apps omit it and stop paying the roll-up (ADR-0053).
    "relay_diagnostics",
];

/// ADR-0053 — declare Chirp's static Tier-2 built-in projection consumption set
/// on `app`. Idempotent (additive union); call once at app construction, before
/// `nmp_app_start`. A null `app` is a silent no-op (D6).
///
/// # Safety
/// `app` must be a valid pointer from `nmp_app_new()` (or null).
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_declare_consumed_projections(app: *mut NmpApp) {
    // Reuse the generic FFI declaration seam: marshal the static &str list into
    // the C-ABI array shape it expects. Building the array here (rather than
    // calling the inherent Rust method) keeps a single code path with the
    // generic seam and exercises the same null/marshalling contract.
    let cstrings: Vec<std::ffi::CString> = CHIRP_CONSUMED_BUILTIN_PROJECTIONS
        .iter()
        .filter_map(|k| std::ffi::CString::new(*k).ok())
        .collect();
    let ptrs: Vec<*const c_char> = cstrings.iter().map(|c| c.as_ptr()).collect();
    nmp_ffi::nmp_app_declare_consumed_projections(app, ptrs.as_ptr(), ptrs.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Single-source-of-truth pin: every key Chirp declares MUST be a kernel
    /// built-in. A producer-side rename of a `KERNEL_BUILTIN_PROJECTION_KEYS`
    /// entry that ships without updating this list fails here (the renamed
    /// built-in would silently stop reaching Chirp once Chirp narrows).
    #[test]
    fn every_chirp_declared_key_is_a_kernel_builtin() {
        let builtins: std::collections::BTreeSet<&str> = nmp_core::KERNEL_BUILTIN_PROJECTION_KEYS
            .iter()
            .copied()
            .collect();
        let stray: Vec<&str> = CHIRP_CONSUMED_BUILTIN_PROJECTIONS
            .iter()
            .copied()
            .filter(|k| !builtins.contains(k))
            .collect();
        assert!(
            stray.is_empty(),
            "Chirp declares projection keys that are NOT kernel built-ins (Tier-1 \
             keys self-gate by registration and must not be declared here; a \
             renamed built-in must be updated in both places): {stray:?}"
        );
    }

    /// Guard against an empty declaration (which would mean "no narrowing" and
    /// silently defeat the ADR-0053 optimization for Chirp).
    #[test]
    fn chirp_declares_a_non_trivial_set() {
        assert!(
            CHIRP_CONSUMED_BUILTIN_PROJECTIONS.len() >= 10,
            "Chirp's declared set collapsed — it would no longer narrow"
        );
    }
}
