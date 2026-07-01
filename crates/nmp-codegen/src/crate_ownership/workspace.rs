use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::crate_ownership_parse::descriptor_for_package;

use super::audit::audit_descriptors;
use super::{OwnershipAuditIssue, OwnershipWorkspace};

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
