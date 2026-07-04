//! #1723 — fail-closed cross-checks proving the [`PROJECTION_CONTRACT`] is the
//! single source for the neutral projection facts. Every Swift registry entry,
//! every keyed-projection entry, and every kernel built-in key must resolve to
//! exactly ONE contract row carrying identical `key` / `schema_id` /
//! `file_identifier`. A registry entry with no contract row (or a mismatched
//! neutral fact) fails here at commit time — there is no parallel path.

use super::*;
use crate::swift_projections_registry::{KEYED_PROJECTIONS, SNAPSHOT_PROJECTIONS};

/// Every projection key is unique in the contract.
#[test]
fn contract_keys_are_unique() {
    let mut seen = std::collections::BTreeSet::new();
    for c in PROJECTION_CONTRACT {
        assert!(seen.insert(c.key), "duplicate contract key {:?}", c.key);
    }
}

/// Tier-1 host registrations carry NO revision dependencies and presence
/// `None`; Tier-2 built-ins carry a non-empty dependency list. This pins the
/// invariant the kernel relies on (only built-ins are revision-tracked).
#[test]
fn tier_and_revision_semantics_are_consistent() {
    for c in PROJECTION_CONTRACT {
        match c.tier {
            ProjectionTier::HostRegistered => {
                assert!(
                    c.dependency_versions.is_empty(),
                    "Tier-1 host registration {:?} must have no source-version deps",
                    c.key
                );
                assert_eq!(
                    c.presence_policy,
                    PresencePolicy::None,
                    "Tier-1 host registration {:?} must have presence None",
                    c.key
                );
                assert_eq!(
                    c.declaration_policy,
                    DeclarationPolicy::RegistrationGated,
                    "Tier-1 {:?} must be RegistrationGated",
                    c.key
                );
            }
            ProjectionTier::KernelBuiltin => {
                assert!(
                    !c.dependency_versions.is_empty(),
                    "Tier-2 built-in {:?} must declare ≥1 source-version dep",
                    c.key
                );
                assert_ne!(
                    c.presence_policy,
                    PresencePolicy::None,
                    "Tier-2 built-in {:?} must declare a presence policy",
                    c.key
                );
                assert_eq!(
                    c.declaration_policy,
                    DeclarationPolicy::SelfDeclaredBuiltin,
                    "Tier-2 {:?} must be SelfDeclaredBuiltin",
                    c.key
                );
            }
        }
    }
}

/// FAIL-CLOSED: every Swift `SNAPSHOT_PROJECTIONS` entry's `key` MUST resolve to
/// a [`PROJECTION_CONTRACT`] row whose `key` is identical. The Swift registry no
/// longer OWNS any neutral fact — `schema_id` / `file_identifier` were removed in
/// #1723 (sourced from the contract), and item-2 collapsed the once-duplicated
/// `json_key` + `TypedSidecar::key` spellings onto this single `key`. This test
/// proves the binding (`entry.key` → contract row) the host-decoder generator
/// relies on is total: a registry entry whose key has no contract row fails here
/// via `contract_for`'s panic.
#[test]
fn swift_registry_keys_resolve_to_contract() {
    for entry in SNAPSHOT_PROJECTIONS {
        // Fail-closed: panics if the key has no contract row.
        let contract = contract_for(entry.key);
        assert_eq!(
            contract.key, entry.key,
            "contract lookup for {:?} returned a row keyed {:?}",
            entry.key, contract.key
        );
    }
}

/// FAIL-CLOSED (item-2, the contract→registry direction): the set of projection
/// keys the Swift presentation registry covers MUST equal the set of contract
/// keys that are Swift-presented — every contract row that is NOT one of the
/// known non-presented exceptions has a `SNAPSHOT_PROJECTIONS` entry, and the
/// registry carries no key absent from the contract. This is the drift gate that
/// makes the registry's projection SET unable to silently diverge from the
/// contract: add a contract entry that should surface in the iOS shell and forget
/// the registry row (or vice versa) and this test fails at commit time.
///
/// The non-presented contract keys are the ones that deliberately have NO
/// whole-value Swift `SnapshotProjections` field:
/// - `signed_events` — the D13 sign-and-return drain, consumed out-of-band (no
///   shell decoder), per its `PROJECTION_CONTRACT` provenance note.
/// - `refs.profile` / `refs.event` — the keyed row-delta carriers, served by the
///   SEPARATE `KEYED_PROJECTIONS` registry (the `keyed_and_snapshot_registries_are_disjoint`
///   test enforces they never appear in `SNAPSHOT_PROJECTIONS`).
/// - `nmp.nip29.group_roster` /
///   `nmp.nip25.reactions` / `nmp.nip51.mute_list` / `nmp.nip51.bookmarks`
///   / `nmp.nip23.articles` / `nmp.wot.bootstrap` / `nmp.notifications`
///   — runtime-owned outputs registered by `nmp-nip29` / `nmp-nip25` /
///   `nmp-nip51` / `nmp-nip23` / `nmp-wot` / `nmp-browser-runtime` for the web
///   and other hosts but with no iOS Swift `SnapshotProjections` consumer field.
///   They are real contract entries but are not yet wired into the Swift
///   presentation registry; add a `SNAPSHOT_PROJECTIONS` row and drop them from
///   this list when the iOS shell starts consuming them.
#[test]
fn swift_presented_contract_keys_match_registry() {
    // Contract keys that intentionally carry no whole-value Swift presentation.
    const NOT_SWIFT_PRESENTED: &[&str] = &[
        "signed_events",
        "refs.profile",
        "refs.event",
        "nmp.nip29.group_roster",
        // The group-scoped NIP-25 reaction aggregate: a Tier-1 sidecar 29er
        // decodes directly (N25A); no iOS Swift `SnapshotProjections` field yet.
        "nmp.nip25.reactions",
        "nmp.nip51.mute_list",
        "nmp.nip51.bookmarks",
        "nmp.nip23.articles",
        "nmp.wot.bootstrap",
        "nmp.notifications",
        "nmp.chat.presence",
        // The merged multi-backend wallet projection (#2915): a Tier-1 sidecar
        // (NWMP) registered by nmp-wallet with no iOS Swift `SnapshotProjections`
        // field yet. Hosts that want it decode the typed payload directly.
        "wallet.merged",
    ];

    let registry_keys: std::collections::BTreeSet<&str> =
        SNAPSHOT_PROJECTIONS.iter().map(|e| e.key).collect();
    let keyed_keys: std::collections::BTreeSet<&str> =
        KEYED_PROJECTIONS.iter().map(|e| e.projection_key).collect();

    // Direction 1 — every Swift-presented contract key has a registry entry.
    for c in PROJECTION_CONTRACT {
        if NOT_SWIFT_PRESENTED.contains(&c.key) {
            // These must NOT be in the whole-value registry (the keyed pair is
            // covered by KEYED_PROJECTIONS; signed_events is out-of-band).
            assert!(
                !registry_keys.contains(c.key),
                "contract key {:?} is marked NOT_SWIFT_PRESENTED but appears in \
                 SNAPSHOT_PROJECTIONS",
                c.key
            );
            continue;
        }
        assert!(
            registry_keys.contains(c.key),
            "contract key {:?} has no SNAPSHOT_PROJECTIONS entry — every contract \
             projection that is not in NOT_SWIFT_PRESENTED (signed_events / refs.*) \
             must have a Swift presentation row, or this gate is stale. Add the \
             registry entry or extend NOT_SWIFT_PRESENTED with a justification.",
            c.key
        );
    }

    // Direction 2 — every registry key is a contract key (no registry-only keys).
    // `swift_registry_keys_resolve_to_contract` already proves each resolves; this
    // also pins that none is a stray that should have been keyed instead.
    for key in &registry_keys {
        assert!(
            lookup(key).is_some(),
            "registry key {key:?} has no contract row"
        );
        assert!(
            !keyed_keys.contains(key),
            "registry key {key:?} is also a KEYED_PROJECTIONS key"
        );
    }
}

/// FAIL-CLOSED: every keyed `KEYED_PROJECTIONS` entry's `file_identifier` (the
/// only neutral identity the keyed registry still owns — the dead `schema_id`
/// field was deleted in #1723) matches its contract row, and the key is a
/// kernel built-in.
#[test]
fn keyed_registry_neutral_facts_match_contract() {
    for entry in KEYED_PROJECTIONS {
        let contract = contract_for(entry.projection_key);
        assert_eq!(
            contract.file_identifier, entry.file_identifier,
            "file_identifier drift for keyed {:?}",
            entry.projection_key
        );
        assert_eq!(
            contract.tier,
            ProjectionTier::KernelBuiltin,
            "keyed projection {:?} must be a kernel built-in",
            entry.projection_key
        );
    }
}

/// Lock the derived kernel built-in key set. This is the set `nmp-core`'s
/// generated `KERNEL_BUILTIN_PROJECTION_KEYS` mirrors (13 decodable Tier-2
/// entries + `signed_events` + the two `refs.*` carriers = 16, sorted).
#[test]
fn kernel_builtin_projection_keys_is_locked() {
    assert_eq!(
        kernel_builtin_projection_keys(),
        vec![
            "accounts",
            "action_lifecycle",
            "action_results",
            "action_stages",
            "active_account",
            "configured_relays",
            "outbox_summary",
            "profile",
            "publish_outbox",
            "publish_queue",
            "refs.event",
            "refs.profile",
            "relay_diagnostics",
            "relay_role_options",
            "settings_hub",
            "signed_events",
        ],
        "kernel built-in projection key set drifted — regenerate \
         builtin_projection_keys.generated.rs (`nmp gen builtin-keys`) and review"
    );
}

/// `contract_for` fails closed on an unknown key.
#[test]
#[should_panic(expected = "no ProjectionContract entry")]
fn contract_for_unknown_key_panics() {
    let _ = contract_for("definitely.not.a.projection");
}
