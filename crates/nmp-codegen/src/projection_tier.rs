//! ADR-0053 / Workstream-E4 — projection-tier classification + the codegen-derived
//! kernel built-in projection key set.
//!
//! The `nmp-codegen` projection registry ([`SNAPSHOT_PROJECTIONS`]) is the single
//! source of truth for the projection keyspace. This module tags each entry's
//! *role* ([`ProjectionTier`]) so the kernel-built-in key set
//! ([`kernel_builtin_projection_keys`]) can be derived FROM the registry rather
//! than hand-maintained in `nmp-core` (and re-hand-maintained in the Chirp app
//! crate). [`crate::rust_builtin_keys`] renders that list into the generated
//! `nmp-core` const.

use crate::swift_projections_registry::SNAPSHOT_PROJECTIONS;

/// Projection-tier classification for a [`SNAPSHOT_PROJECTIONS`] entry.
///
/// #1610: the former `Transient` variant (JSON-era keys with no typed sidecar:
/// `timeline`, `inserted`, `updated`, `removed`, `last_action_result`) was
/// deleted because those five entries no longer exist in the registry.
/// The coverage gate in `swift_projections_registry_tests::typed_sidecar_coverage_gate`
/// now prevents new sidecar-less entries from accumulating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionTier {
    /// Tier-2 **kernel-owned built-in**: emitted directly by
    /// `Kernel::snapshot_projections_with_publish_cluster` and gated by the
    /// host-declared consumed-projection set (ADR-0053). These are exactly the
    /// keys `nmp_core::KERNEL_BUILTIN_PROJECTION_KEYS` enumerates.
    KernelBuiltin,
    /// Tier-1 **host/protocol-registered** projection: produced by a
    /// `SnapshotRegistry::register*` closure (wallet, the NIP crates, Marmot,
    /// the op-feed, the embed sidecar). These self-gate by registration —
    /// registration *is* the declaration — and are never members of the
    /// kernel built-in set.
    HostRegistered,
}

/// Classify a [`SNAPSHOT_PROJECTIONS`] `json_key` into its [`ProjectionTier`].
///
/// This match is the registry-local SSOT for *which* registry entries are
/// Tier-2 kernel-owned built-ins. Adding a registry entry without a matching
/// arm trips the catch-all panic (and the `every_registry_key_is_classified`
/// test fails at commit time), so the classification cannot silently drift from
/// the registry data it tags.
#[must_use]
pub fn projection_tier(json_key: &str) -> ProjectionTier {
    match json_key {
        // ── Tier-2 kernel-owned built-ins (decodable; have a registry entry) ──
        "publish_queue"
        | "publish_outbox"
        | "outbox_summary"
        | "configured_relays"
        | "relay_role_options"
        | "settings_hub"
        | "action_results"
        | "action_stages"
        | "action_lifecycle"
        | "accounts"
        | "active_account"
        | "profile"
        | "relay_diagnostics"
        | "claimed_profiles"
        | "claimed_events"
        | "resolved_profiles" => ProjectionTier::KernelBuiltin,
        // ── Tier-1 host/protocol registrations (self-gate by registration) ──
        "wallet"
        | "bunker_handshake"
        | "nip46_onboarding"
        | "signer_state"
        | "nmp.feed.home"
        | "nmp.follow_list"
        | "nmp.nip29.group_chat"
        | "nmp.nip29.discovered_groups"
        | "nmp.nip29.group_defaults"
        | "nmp.nip17.dm_inbox"
        | "nmp.nip17.dm_relay_list"
        | "nmp.nip57.zaps"
        | "claimed_event_embeds"
        | "nmp.marmot.snapshot"
        | "nmp.marmot.messages" => ProjectionTier::HostRegistered,
        other => panic!(
            "unclassified SNAPSHOT_PROJECTIONS json_key {other:?} — add it to \
             `projection_tier` (Tier-2 built-in or Tier-1 host registration). \
             The kernel built-in key set is derived from this classification, \
             so an unclassified key cannot be generated."
        ),
    }
}

/// Tier-2 kernel-owned built-in projection keys the kernel emits but for which
/// **no codegen shell decoder is generated** — they have no [`SNAPSHOT_PROJECTIONS`]
/// entry because they are consumed out-of-band, not decoded into the generated
/// `SnapshotProjections` struct:
///
/// - `signed_events` — the D13 sign-and-return drain. The host resumes its
///   `signEventForReturn` continuation by `correlation_id` through the FFI
///   layer; there is no `SnapshotProjections` field for it.
/// - `mention_profiles` — Android decodes it via a hand-written decoder outside
///   the codegen registry; Swift reads the merged `resolved_profiles` map
///   instead. The kernel still emits it as a building block, so it is a
///   built-in the `consume_all` set must include.
/// - `refs.profile` / `refs.event` (ADR-0063 #1671) — the two keyed row-delta
///   carriers. Each ships an opaque NRRD per-key row-delta batch consumed by the
///   host `RefRowCache`, not a `SnapshotProjections` JSON field, so neither has a
///   generated shell decoder.
///
/// These are the ONLY four members of [`kernel_builtin_projection_keys`] that are
/// not also `SNAPSHOT_PROJECTIONS` entries. They are pinned by the
/// `kernel_builtins_without_shell_decoder_are_not_in_registry` test so the list
/// cannot silently overlap the decoder registry.
pub const KERNEL_BUILTINS_WITHOUT_SHELL_DECODER: &[&str] = &[
    "signed_events",
    "mention_profiles",
    // ADR-0063 (#1671 integration glue) — the two keyed row-delta carriers
    // (`refs.profile` / `refs.event`). They are kernel-emitted Tier-2 built-ins
    // but carry an opaque NRRD per-key row-delta batch consumed by the host
    // `RefRowCache`, NOT a `SnapshotProjections` JSON field — so, like
    // `signed_events`, they have no shell decoder in the registry. Registered
    // here so they enter `KERNEL_BUILTIN_PROJECTION_KEYS` (and thus the manifest
    // / oracle / consume-all set) without a phantom Swift field.
    "refs.profile",
    "refs.event",
];

/// The single source of truth for the **Tier-2 kernel-owned built-in projection
/// key set** — the codegen-derived list that `nmp-core`'s
/// `KERNEL_BUILTIN_PROJECTION_KEYS` is generated from (and therefore the set the
/// kernel's `consume_all_builtin_projections` covers).
///
/// Composed from the registry itself: every [`SNAPSHOT_PROJECTIONS`] entry
/// tagged [`ProjectionTier::KernelBuiltin`] by [`projection_tier`], unioned with
/// the [`KERNEL_BUILTINS_WITHOUT_SHELL_DECODER`] keys (kernel-emitted but not
/// shell-decoded). Returned **sorted + deduplicated** for a deterministic
/// generated artifact.
#[must_use]
pub fn kernel_builtin_projection_keys() -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = SNAPSHOT_PROJECTIONS
        .iter()
        .filter(|e| projection_tier(e.json_key) == ProjectionTier::KernelBuiltin)
        .map(|e| e.json_key)
        .chain(KERNEL_BUILTINS_WITHOUT_SHELL_DECODER.iter().copied())
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every registry `json_key` must be classified by [`projection_tier`]. A new
    /// entry without a matching arm trips the catch-all panic here at commit time,
    /// so the classification (and therefore the derived kernel built-in set)
    /// cannot silently drift from the registry data.
    #[test]
    fn every_registry_key_is_classified() {
        for entry in SNAPSHOT_PROJECTIONS {
            let _ = projection_tier(entry.json_key);
        }
    }

    /// The two kernel built-ins with no shell decoder MUST NOT have a registry
    /// entry — they are the documented exception (consumed out-of-band), so an
    /// accidental registry entry (which would generate a phantom Swift field) is
    /// flagged here.
    #[test]
    fn kernel_builtins_without_shell_decoder_are_not_in_registry() {
        let registry: std::collections::BTreeSet<&str> =
            SNAPSHOT_PROJECTIONS.iter().map(|e| e.json_key).collect();
        for key in KERNEL_BUILTINS_WITHOUT_SHELL_DECODER {
            assert!(
                !registry.contains(key),
                "{key:?} is in KERNEL_BUILTINS_WITHOUT_SHELL_DECODER but ALSO has a \
                 SNAPSHOT_PROJECTIONS entry — it would be double-counted in \
                 kernel_builtin_projection_keys"
            );
        }
    }

    /// Lock the derived kernel built-in set: 16 Tier-2 registry entries + the 4
    /// out-of-registry built-ins (`signed_events`, `mention_profiles`, and the
    /// two ADR-0063 `refs.*` row-delta carriers) = 20, sorted + deduplicated.
    /// This is the set `nmp-core`'s generated `KERNEL_BUILTIN_PROJECTION_KEYS`
    /// mirrors.
    #[test]
    fn kernel_builtin_projection_keys_is_locked() {
        let keys = kernel_builtin_projection_keys();
        assert_eq!(
            keys,
            vec![
                "accounts",
                "action_lifecycle",
                "action_results",
                "action_stages",
                "active_account",
                "claimed_events",
                "claimed_profiles",
                "configured_relays",
                "mention_profiles",
                "outbox_summary",
                "profile",
                "publish_outbox",
                "publish_queue",
                "refs.event",
                "refs.profile",
                "relay_diagnostics",
                "relay_role_options",
                "resolved_profiles",
                "settings_hub",
                "signed_events",
            ],
            "kernel built-in projection key set drifted — regenerate \
             builtin_projection_keys.generated.rs (`nmp gen builtin-keys`) and review"
        );
    }

    /// Confirm the `Transient` variant is gone and no remaining entry would
    /// have matched it. This test exists to fail loudly if someone tries to
    /// re-introduce a transient-tier key: the coverage gate in
    /// `swift_projections_registry_tests::typed_sidecar_coverage_gate` is the
    /// permanent enforcement, but this companion test makes the classifier
    /// exhaustive-match requirement explicit.
    #[test]
    fn no_unclassified_registry_key_exists() {
        // `projection_tier` panics on any unknown key, so iterating the full
        // registry is sufficient — the test harness turns the panic into a
        // test failure.
        for entry in SNAPSHOT_PROJECTIONS {
            let tier = projection_tier(entry.json_key);
            assert!(
                tier == ProjectionTier::KernelBuiltin || tier == ProjectionTier::HostRegistered,
                "key {:?} resolved to an unexpected tier — update `projection_tier`",
                entry.json_key
            );
        }
    }
}
