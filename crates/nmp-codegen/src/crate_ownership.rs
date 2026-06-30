//! Workspace ownership report + audit support for `nmp crate-ownership`.
//!
//! Descriptors live in each crate's Rust source via
//! `nmp_ownership::declare_crate_ownership!`. This module only discovers and
//! validates the active Cargo workspace; it is not a hand-maintained registry.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::crate_ownership_parse::descriptor_for_package;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnershipQuery {
    pub crate_filter: Option<String>,
    pub scope_kind: Option<String>,
    pub scope_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OwnershipWorkspace {
    pub workspace_root: PathBuf,
    pub descriptors: Vec<OwnershipDescriptor>,
    pub audit_issues: Vec<OwnershipAuditIssue>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OwnershipDescriptor {
    pub owner_id: String,
    pub crate_name: String,
    pub summary: String,
    pub claims: Vec<OwnershipClaim>,
    pub notes: Vec<OwnershipNote>,
    pub source_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OwnershipClaim {
    pub claim_type: String,
    pub id: String,
    pub exclusive: bool,
    pub scope_kind: String,
    pub scope_value: String,
    pub context: String,
    pub owns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OwnershipNote {
    pub claim: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OwnershipAuditIssue {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    workspace_members: Vec<String>,
    workspace_root: PathBuf,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    id: String,
    name: String,
    manifest_path: PathBuf,
    source: Option<String>,
}

#[must_use]
pub fn render_ownership_tsv(workspace: &OwnershipWorkspace, query: &OwnershipQuery) -> String {
    let mut out = String::new();
    for descriptor in filtered_descriptors(workspace, query) {
        if query.scope_kind.is_none() && query.scope_value.is_none() {
            push_tsv_row(
                &mut out,
                &[
                    "crate",
                    &descriptor.crate_name,
                    &descriptor.owner_id,
                    &descriptor.summary,
                ],
            );
        }
        for claim in descriptor
            .claims
            .iter()
            .filter(|claim| claim_matches(claim, query))
        {
            push_tsv_row(
                &mut out,
                &[
                    "owns",
                    &descriptor.crate_name,
                    &claim.claim_type,
                    &claim.id,
                    &claim.scope_kind,
                    &claim.scope_value,
                    &claim.context,
                    if claim.exclusive {
                        "exclusive"
                    } else {
                        "shared"
                    },
                ],
            );
        }
        if query.scope_kind.is_none() && query.scope_value.is_none() {
            for note in &descriptor.notes {
                push_tsv_row(
                    &mut out,
                    &["note", &descriptor.crate_name, &note.claim, &note.text],
                );
            }
        }
    }
    out
}

#[must_use]
pub fn render_ownership_human(workspace: &OwnershipWorkspace, query: &OwnershipQuery) -> String {
    let mut out = String::new();
    for descriptor in filtered_descriptors(workspace, query) {
        out.push_str(&format!(
            "{} ({})\n  {}\n",
            descriptor.crate_name, descriptor.owner_id, descriptor.summary
        ));
        let mut wrote_claim = false;
        for claim in descriptor
            .claims
            .iter()
            .filter(|claim| claim_matches(claim, query))
        {
            wrote_claim = true;
            let context = if claim.context.is_empty() {
                String::new()
            } else {
                format!(" context={}", claim.context)
            };
            out.push_str(&format!(
                "  owns {} {}: {}={}{} ({})\n",
                claim.claim_type,
                claim.id,
                claim.scope_kind,
                claim.scope_value,
                context,
                if claim.exclusive {
                    "exclusive"
                } else {
                    "shared"
                }
            ));
            for item in &claim.owns {
                out.push_str(&format!("    - {item}\n"));
            }
        }
        if !wrote_claim && descriptor.claims.is_empty() && query.scope_kind.is_none() {
            out.push_str("  owns no protected semantics\n");
        }
        if query.scope_kind.is_none() && query.scope_value.is_none() {
            for note in &descriptor.notes {
                out.push_str(&format!("  note {}: {}\n", note.claim, note.text));
            }
        }
    }
    out
}

pub fn render_ownership_json(workspace: &OwnershipWorkspace) -> Result<String, String> {
    serde_json::to_string_pretty(workspace).map_err(|err| err.to_string())
}

pub fn load_workspace_ownership(
    manifest_path: Option<&Path>,
) -> Result<OwnershipWorkspace, String> {
    let metadata = cargo_metadata(manifest_path)?;
    let workspace_ids: BTreeSet<&str> = metadata
        .workspace_members
        .iter()
        .map(String::as_str)
        .collect();
    let packages = metadata
        .packages
        .iter()
        .filter(|package| package.source.is_none() && workspace_ids.contains(package.id.as_str()))
        .collect::<Vec<_>>();

    let mut descriptors = Vec::new();
    let mut audit_issues = Vec::new();
    for package in packages {
        match descriptor_for_package(&package.name, &package.manifest_path) {
            Ok(Some(descriptor)) => descriptors.push(descriptor),
            Ok(None) => audit_issues.push(OwnershipAuditIssue {
                code: "NMP-OWNERSHIP-MISSING".to_string(),
                message: format!(
                    "{} has no declare_crate_ownership! descriptor",
                    package.name
                ),
            }),
            Err(message) => audit_issues.push(OwnershipAuditIssue {
                code: "NMP-OWNERSHIP-PARSE".to_string(),
                message: format!("{}: {message}", package.name),
            }),
        }
    }
    descriptors.sort_unstable_by(|a, b| a.crate_name.cmp(&b.crate_name));
    audit_issues.extend(audit_descriptors(&descriptors));
    Ok(OwnershipWorkspace {
        workspace_root: metadata.workspace_root,
        descriptors,
        audit_issues,
    })
}

fn cargo_metadata(manifest_path: Option<&Path>) -> Result<CargoMetadata, String> {
    let mut command = Command::new("cargo");
    command.args(["metadata", "--no-deps", "--format-version", "1"]);
    if let Some(path) = manifest_path {
        command.arg("--manifest-path").arg(path);
    }
    let output = command
        .output()
        .map_err(|err| format!("failed to run cargo metadata: {err}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    serde_json::from_slice(&output.stdout).map_err(|err| format!("invalid cargo metadata: {err}"))
}

fn audit_descriptors(descriptors: &[OwnershipDescriptor]) -> Vec<OwnershipAuditIssue> {
    let mut issues = Vec::new();
    let mut crate_names = BTreeSet::new();
    let mut exclusive_scopes: BTreeMap<String, Vec<String>> = BTreeMap::new();
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
    issues
}

fn filtered_descriptors<'a>(
    workspace: &'a OwnershipWorkspace,
    query: &'a OwnershipQuery,
) -> impl Iterator<Item = &'a OwnershipDescriptor> {
    workspace.descriptors.iter().filter(move |descriptor| {
        query
            .crate_filter
            .as_ref()
            .map_or(true, |name| &descriptor.crate_name == name)
            && (query.scope_kind.is_none()
                || descriptor
                    .claims
                    .iter()
                    .any(|claim| claim_matches(claim, query)))
    })
}

fn claim_matches(claim: &OwnershipClaim, query: &OwnershipQuery) -> bool {
    query
        .scope_kind
        .as_ref()
        .map_or(true, |kind| &claim.scope_kind == kind)
        && query
            .scope_value
            .as_ref()
            .map_or(true, |value| &claim.scope_value == value)
}

fn push_tsv_row(out: &mut String, fields: &[&str]) {
    out.push_str(
        &fields
            .iter()
            .map(|f| f.replace(['\t', '\n'], " "))
            .collect::<Vec<_>>()
            .join("\t"),
    );
    out.push('\n');
}
