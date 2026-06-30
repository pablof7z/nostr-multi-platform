//! Compiled crate-ownership contract gates.
//!
//! These tests keep the ownership report from becoming optional documentation:
//! every active workspace crate must declare a descriptor, and duplicate
//! exclusive scopes are doctrine failures.

use nmp_codegen::{
    load_workspace_ownership, render_ownership_tsv, OwnershipQuery, OwnershipWorkspace,
};

use super::workspace_root;

fn ownership_workspace() -> OwnershipWorkspace {
    let manifest = workspace_root().join("Cargo.toml");
    load_workspace_ownership(Some(&manifest))
        .unwrap_or_else(|err| panic!("load workspace ownership: {err}"))
}

#[test]
fn workspace_ownership_audit_is_clean() {
    let workspace = ownership_workspace();
    assert!(
        workspace.audit_issues.is_empty(),
        "ownership audit must be clean:\n{}",
        workspace
            .audit_issues
            .iter()
            .map(|issue| format!("{}: {}", issue.code, issue.message))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(
        workspace
            .descriptors
            .iter()
            .all(|descriptor| !descriptor.summary.trim().is_empty()),
        "every ownership descriptor must include a one-or-two-line crate summary"
    );
}

#[test]
fn nip29_does_not_claim_chat_artifact_kinds() {
    let workspace = ownership_workspace();
    for kind in ["9", "11"] {
        let query = OwnershipQuery {
            crate_filter: Some("nmp-nip29".to_string()),
            scope_kind: Some("kind".to_string()),
            scope_value: Some(kind.to_string()),
        };
        let tsv = render_ownership_tsv(&workspace, &query);
        assert!(
            tsv.trim().is_empty(),
            "nmp-nip29 must not own kind {kind}; rows:\n{tsv}"
        );
    }
}

#[test]
fn planner_owns_mechanisms_not_event_kinds() {
    let workspace = ownership_workspace();
    let planner = workspace
        .descriptors
        .iter()
        .find(|descriptor| descriptor.crate_name == "nmp-planner")
        .expect("nmp-planner descriptor must exist");
    assert!(
        planner
            .claims
            .iter()
            .all(|claim| claim.scope_kind != "kind"),
        "nmp-planner must not claim event kinds"
    );
    assert!(
        planner.claims.iter().any(|claim| {
            claim.claim_type == "mechanism"
                && claim.scope_kind == "field"
                && claim.scope_value == "relay_pin"
        }),
        "nmp-planner must own the relay_pin mechanism"
    );
}

#[test]
fn legacy_relations_crate_claims_no_engagement_semantics() {
    let workspace = ownership_workspace();
    let relations = workspace
        .descriptors
        .iter()
        .find(|descriptor| descriptor.crate_name == "nmp-relations")
        .expect("nmp-relations descriptor must exist while the legacy crate remains");
    assert!(
        relations.claims.is_empty(),
        "nmp-relations is a legacy compatibility adapter, not an engagement owner; claims: {:?}",
        relations.claims
    );
}
