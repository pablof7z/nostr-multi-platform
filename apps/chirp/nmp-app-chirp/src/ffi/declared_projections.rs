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
    ///
    /// This is direction 1 of the bidirectional drift gate (ADR-0053 DEBT 1):
    ///   declared ⊆ builtins — no stray non-builtin keys in the declared set.
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

    /// Bidirectional drift gate — direction 2 (ADR-0053 DEBT 1):
    ///   codegen-decoded builtins ⊆ declared.
    ///
    /// Every Tier-2 kernel built-in key that has an entry in the `nmp-codegen`
    /// `SNAPSHOT_PROJECTIONS` registry (i.e. Chirp generates a Swift decoder for
    /// it) MUST be present in `CHIRP_CONSUMED_BUILTIN_PROJECTIONS`. A new Tier-2
    /// built-in added to the codegen registry without updating the declared set
    /// would compile and pass direction-1, but the key would silently go dark the
    /// moment Chirp starts declaring its set — caught here at commit time.
    ///
    /// Keys that have Android-only decoders outside the codegen registry (e.g.
    /// `mention_profiles`) are exempt from this check — they are covered by
    /// direction 1 (they must still be in `KERNEL_BUILTIN_PROJECTION_KEYS`).
    #[test]
    fn every_codegen_decoded_builtin_is_declared() {
        let builtins: std::collections::BTreeSet<&str> = nmp_core::KERNEL_BUILTIN_PROJECTION_KEYS
            .iter()
            .copied()
            .collect();
        let declared: std::collections::BTreeSet<&str> = CHIRP_CONSUMED_BUILTIN_PROJECTIONS
            .iter()
            .copied()
            .collect();
        // The codegen registry keys that are ALSO Tier-2 kernel built-ins — the
        // set of built-ins Chirp has a codegen-generated Swift decoder for.
        let codegen_decoded_builtins: Vec<&str> =
            nmp_codegen::swift_projections_registry::SNAPSHOT_PROJECTIONS
                .iter()
                .map(|e| e.json_key)
                .filter(|k| builtins.contains(k))
                .collect();
        let missing: Vec<&str> = codegen_decoded_builtins
            .iter()
            .copied()
            .filter(|k| !declared.contains(k))
            .collect();
        assert!(
            missing.is_empty(),
            "Tier-2 built-in projection key(s) present in the codegen registry \
             (Chirp generates a Swift decoder for them) but MISSING from \
             CHIRP_CONSUMED_BUILTIN_PROJECTIONS — they would silently go dark \
             once Chirp declares its set (ADR-0053 drift gate, direction 2). \
             Add them to CHIRP_CONSUMED_BUILTIN_PROJECTIONS: {missing:?}"
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
