use std::collections::{BTreeMap, BTreeSet};

use crate::read_model_contract::READ_MODEL_CONTRACT;

use super::{OwnershipAuditIssue, OwnershipDescriptor};

pub(super) fn audit_descriptors(descriptors: &[OwnershipDescriptor]) -> Vec<OwnershipAuditIssue> {
    let mut issues = Vec::new();
    let mut crate_names = BTreeSet::new();
    let mut exclusive_scopes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let nip_namespace_owners: BTreeSet<String> = descriptors
        .iter()
        .filter_map(|descriptor| {
            descriptor
                .owner_id
                .strip_prefix("nmp.nip")
                .filter(|suffix| suffix.chars().all(|c| c.is_ascii_digit()))
                .map(|_| descriptor.owner_id.clone())
        })
        .collect();
    for descriptor in descriptors {
        if descriptor.summary.trim().is_empty() {
            issues.push(OwnershipAuditIssue {
                code: "NMP-OWNERSHIP-SUMMARY".to_string(),
                message: format!("{} has an empty ownership summary", descriptor.crate_name),
            });
        }
        if !crate_names.insert(descriptor.crate_name.clone()) {
            issues.push(OwnershipAuditIssue {
                code: "NMP-OWNERSHIP-DUPLICATE-CRATE".to_string(),
                message: format!("duplicate descriptor for {}", descriptor.crate_name),
            });
        }
        for claim in &descriptor.claims {
            if claim.scope_kind == "action" {
                if let Some(owner_id) = nip_owner_for_action(&claim.scope_value) {
                    if nip_namespace_owners.contains(&owner_id) && descriptor.owner_id != owner_id {
                        issues.push(OwnershipAuditIssue {
                            code: "NMP-OWNERSHIP-NIP-ACTION-OWNER".to_string(),
                            message: format!(
                                "{} claims action {} but {} is the protocol owner",
                                descriptor.crate_name, claim.scope_value, owner_id
                            ),
                        });
                    }
                }
            }
            if claim.exclusive {
                let key = format!(
                    "{}\t{}\t{}\t{}",
                    claim.claim_type, claim.scope_kind, claim.scope_value, claim.context
                );
                exclusive_scopes
                    .entry(key)
                    .or_default()
                    .push(format!("{}:{}", descriptor.crate_name, claim.id));
            }
        }
    }
    for (scope, owners) in exclusive_scopes {
        if owners.len() > 1 {
            issues.push(OwnershipAuditIssue {
                code: "NMP-OWNERSHIP-COLLISION".to_string(),
                message: format!(
                    "exclusive ownership scope {} is claimed by {}",
                    scope.replace('\t', " "),
                    owners.join(", ")
                ),
            });
        }
    }
    issues.extend(audit_read_model_contracts(descriptors));
    issues
}

fn audit_read_model_contracts(descriptors: &[OwnershipDescriptor]) -> Vec<OwnershipAuditIssue> {
    let claim_ids = descriptors
        .iter()
        .flat_map(|descriptor| descriptor.claims.iter().map(|claim| claim.id.as_str()))
        .collect::<BTreeSet<_>>();
    let crate_names = descriptors
        .iter()
        .map(|descriptor| descriptor.crate_name.as_str())
        .collect::<BTreeSet<_>>();
    let mut ids = BTreeSet::new();
    let mut issues = Vec::new();
    for contract in READ_MODEL_CONTRACT {
        if !ids.insert(contract.id) {
            issues.push(OwnershipAuditIssue {
                code: "NMP-READ-MODEL-DUPLICATE-ID".to_string(),
                message: format!("duplicate read-model contract id {}", contract.id),
            });
        }
        if !claim_ids.contains(contract.owner_claim) {
            issues.push(OwnershipAuditIssue {
                code: "NMP-READ-MODEL-OWNER-CLAIM".to_string(),
                message: format!(
                    "read-model {} cites missing owner claim `{}`",
                    contract.id, contract.owner_claim
                ),
            });
        }
        if !crate_names.contains(contract.owner_crate) {
            issues.push(OwnershipAuditIssue {
                code: "NMP-READ-MODEL-OWNER-CRATE".to_string(),
                message: format!(
                    "read-model {} cites missing owner crate `{}`",
                    contract.id, contract.owner_crate
                ),
            });
        }
    }
    issues
}

fn nip_owner_for_action(action: &str) -> Option<String> {
    let rest = action.strip_prefix("nmp.nip")?;
    let digits = rest
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() || !rest[digits.len()..].starts_with('.') {
        return None;
    }
    Some(format!("nmp.nip{digits}"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::super::OwnershipClaim;
    use super::*;

    #[test]
    fn audit_rejects_nip_action_claim_outside_declared_protocol_owner() {
        let descriptors = vec![
            descriptor(
                "nmp.nip51",
                "nmp-nip51",
                vec![claim("artifact", "nostr.nip51.lists", "kind", "10006")],
            ),
            descriptor(
                "nmp.router",
                "nmp-router",
                vec![claim(
                    "namespace",
                    "action.nmp.nip51.block_relay",
                    "action",
                    "nmp.nip51.block_relay",
                )],
            ),
        ];
        let issues = audit_descriptors(&descriptors);
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "NMP-OWNERSHIP-NIP-ACTION-OWNER"),
            "expected NIP action owner issue, got {issues:?}"
        );
    }

    #[test]
    fn audit_allows_legacy_nip_namespace_without_protocol_owner() {
        let descriptors = vec![descriptor(
            "nmp.router",
            "nmp-router",
            vec![claim(
                "namespace",
                "action.nmp.nip65.publish_relay_list",
                "action",
                "nmp.nip65.publish_relay_list",
            )],
        )];
        let issues = audit_descriptors(&descriptors);
        assert!(
            issues
                .iter()
                .all(|issue| issue.code != "NMP-OWNERSHIP-NIP-ACTION-OWNER"),
            "nmp.nip65 has no separate protocol owner; got {issues:?}"
        );
    }

    #[test]
    fn audit_rejects_read_model_contract_owner_claim_without_descriptor_claim() {
        let descriptors = vec![
            descriptor("nmp.router", "nmp-router", Vec::new()),
            descriptor(
                "nmp.nip01",
                "nmp-nip01",
                vec![claim(
                    "artifact",
                    "nostr.kind.0.profile_metadata",
                    "kind",
                    "0",
                )],
            ),
            descriptor(
                "nmp.nip17",
                "nmp-nip17",
                vec![claim(
                    "artifact",
                    "nostr.kind.10050.dm_relay_list",
                    "kind",
                    "10050",
                )],
            ),
        ];
        let issues = audit_read_model_contracts(&descriptors);
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "NMP-READ-MODEL-OWNER-CLAIM"),
            "expected missing read-model owner claim issue, got {issues:?}"
        );
    }

    fn descriptor(
        owner_id: &str,
        crate_name: &str,
        claims: Vec<OwnershipClaim>,
    ) -> OwnershipDescriptor {
        OwnershipDescriptor {
            owner_id: owner_id.to_string(),
            crate_name: crate_name.to_string(),
            summary: "summary".to_string(),
            claims,
            notes: Vec::new(),
            source_path: PathBuf::new(),
        }
    }

    fn claim(claim_type: &str, id: &str, scope_kind: &str, scope_value: &str) -> OwnershipClaim {
        OwnershipClaim {
            claim_type: claim_type.to_string(),
            id: id.to_string(),
            exclusive: true,
            scope_kind: scope_kind.to_string(),
            scope_value: scope_value.to_string(),
            context: String::new(),
            owns: vec!["test ownership".to_string()],
        }
    }
}
