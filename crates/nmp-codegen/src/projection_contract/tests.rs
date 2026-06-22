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

/// FAIL-CLOSED: every Swift `SNAPSHOT_PROJECTIONS` entry's neutral facts (the
/// FAIL-CLOSED: every Swift `SNAPSHOT_PROJECTIONS` entry's producer `key` MUST
/// resolve to a [`PROJECTION_CONTRACT`] row, AND that row's `key` must equal the
/// entry's `json_key` OR carry a deliberate producer-key/json-key split that the
/// contract knows about. The Swift registry no longer OWNS `schema_id` /
/// `file_identifier` (those fields were removed in #1723 — the host-decoder
/// generator sources them from the contract by `key`), so there is nothing to
/// drift; this test proves the binding (`sidecar.key` → contract row) the
/// generator relies on is total. A registry entry whose producer key has no
/// contract row fails here via `contract_for`'s panic.
#[test]
fn swift_registry_keys_resolve_to_contract() {
    for entry in SNAPSHOT_PROJECTIONS {
        let sidecar = entry
            .typed_sidecar
            .as_ref()
            .expect("coverage gate guarantees Some");
        // Fail-closed: panics if the producer key has no contract row.
        let contract = contract_for(sidecar.key);
        // The contract row this entry binds to must be the one keyed by the
        // producer key (identity), so the generator's neutral lookup is correct.
        assert_eq!(
            contract.key, sidecar.key,
            "contract lookup for {:?} returned a row keyed {:?}",
            sidecar.key, contract.key
        );
        // The json_key must ALSO have a contract row (it is the kernel-emitted
        // map key; for most entries json_key == sidecar.key, but the op-feed /
        // follow-list deliberately split them — both must be known).
        let _ = contract_for(entry.json_key);
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
/// generated `KERNEL_BUILTIN_PROJECTION_KEYS` mirrors (14 decodable Tier-2
/// entries + `signed_events` + the two `refs.*` carriers = 17, sorted).
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
            "claimed_events",
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
