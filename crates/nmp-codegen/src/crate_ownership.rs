//! Workspace ownership report + audit support for `nmp crate-ownership`.
//!
//! Descriptors live in each crate's Rust source via
//! `nmp_ownership::declare_crate_ownership!`. This module only discovers and
//! validates the active Cargo workspace; it is not a hand-maintained registry.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::IsTerminal;
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

/// Wrap-column for descriptor summaries. Chosen for readability (roughly the
/// classic 80-column prose measure) rather than the real terminal width,
/// since summaries are long enough to overflow even very wide terminals.
const SUMMARY_WRAP_WIDTH: usize = 88;

#[derive(Clone, Copy)]
struct Palette {
    bold: &'static str,
    dim: &'static str,
    reset: &'static str,
    crate_name: &'static str,
    owner_id: &'static str,
    claim_type: &'static str,
    claim_id: &'static str,
    scope: &'static str,
    context: &'static str,
    exclusive: &'static str,
    shared: &'static str,
    bullet: &'static str,
    note_label: &'static str,
    rule: &'static str,
}

const COLOR_PALETTE: Palette = Palette {
    bold: "\x1b[1m",
    dim: "\x1b[2m",
    reset: "\x1b[0m",
    crate_name: "\x1b[1;96m",
    owner_id: "\x1b[2;37m",
    claim_type: "\x1b[35m",
    claim_id: "\x1b[1;97m",
    scope: "\x1b[33m",
    context: "\x1b[2;3m",
    exclusive: "\x1b[1;31m",
    shared: "\x1b[32m",
    bullet: "\x1b[36m",
    note_label: "\x1b[1;33m",
    rule: "\x1b[2;34m",
};

const PLAIN_PALETTE: Palette = Palette {
    bold: "",
    dim: "",
    reset: "",
    crate_name: "",
    owner_id: "",
    claim_type: "",
    claim_id: "",
    scope: "",
    context: "",
    exclusive: "",
    shared: "",
    bullet: "",
    note_label: "",
    rule: "",
};

fn active_palette() -> &'static Palette {
    let colorize = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    if colorize {
        &COLOR_PALETTE
    } else {
        &PLAIN_PALETTE
    }
}

/// Greedily word-wraps `text` to `width` columns, indenting every line
/// (including the first) with `indent`.
fn wrap_text(text: &str, width: usize, indent: &str) -> String {
    let budget = width.saturating_sub(indent.chars().count()).max(1);
    let mut out = String::new();
    let mut line_len = 0usize;
    let mut first_word_on_line = true;
    out.push_str(indent);
    for word in text.split_whitespace() {
        let word_len = word.chars().count();
        if !first_word_on_line && line_len + 1 + word_len > budget {
            out.push('\n');
            out.push_str(indent);
            line_len = 0;
            first_word_on_line = true;
        }
        if !first_word_on_line {
            out.push(' ');
            line_len += 1;
        }
        out.push_str(word);
        line_len += word_len;
        first_word_on_line = false;
    }
    out.push('\n');
    out
}

/// A run of claims that are identical except for `scope_kind`/`scope_value`/
/// `context` (e.g. the same protocol-artifact responsibility declared once
/// per Nostr event kind). Grouping these for display avoids repeating the
/// same claim type, id, exclusivity, and `owns` text once per scope value.
struct ClaimGroup<'a> {
    claim_type: &'a str,
    id: &'a str,
    exclusive: bool,
    owns: &'a [String],
    scopes: Vec<(&'a str, &'a str, &'a str)>,
}

fn group_claims<'a>(claims: impl Iterator<Item = &'a OwnershipClaim>) -> Vec<ClaimGroup<'a>> {
    let mut groups: Vec<ClaimGroup<'a>> = Vec::new();
    for claim in claims {
        let existing = groups.iter_mut().find(|group| {
            group.claim_type == claim.claim_type
                && group.id == claim.id
                && group.exclusive == claim.exclusive
                && group.owns == claim.owns.as_slice()
        });
        let scope = (
            claim.scope_kind.as_str(),
            claim.scope_value.as_str(),
            claim.context.as_str(),
        );
        match existing {
            Some(group) => group.scopes.push(scope),
            None => groups.push(ClaimGroup {
                claim_type: &claim.claim_type,
                id: &claim.id,
                exclusive: claim.exclusive,
                owns: &claim.owns,
                scopes: vec![scope],
            }),
        }
    }
    groups
}

#[must_use]
pub fn render_ownership_human(workspace: &OwnershipWorkspace, query: &OwnershipQuery) -> String {
    let p = active_palette();
    let mut out = String::new();
    let mut first = true;
    for descriptor in filtered_descriptors(workspace, query) {
        if !first {
            out.push('\n');
        }
        first = false;

        out.push_str(&format!(
            "{rule}::{reset} {cn}{crate_name}{reset} {dim}({owner_id}){reset}\n",
            rule = p.rule,
            reset = p.reset,
            cn = p.crate_name,
            crate_name = descriptor.crate_name,
            dim = p.owner_id,
            owner_id = descriptor.owner_id,
        ));
        out.push_str(&wrap_text(&descriptor.summary, SUMMARY_WRAP_WIDTH, "    "));

        let mut wrote_claim = false;
        let claims = descriptor
            .claims
            .iter()
            .filter(|claim| claim_matches(claim, query));
        for group in group_claims(claims) {
            wrote_claim = true;
            let badge = if group.exclusive {
                format!("{}{}EXCLUSIVE{}", p.bold, p.exclusive, p.reset)
            } else {
                format!("{}shared{}", p.shared, p.reset)
            };
            out.push_str(&format!(
                "\n  {bullet}\u{25cf}{reset} {ct}{claim_type}{reset} {ci}{id}{reset}  [{badge}]\n",
                bullet = p.bullet,
                reset = p.reset,
                ct = p.claim_type,
                claim_type = group.claim_type,
                ci = p.claim_id,
                id = group.id,
                badge = badge,
            ));
            for (scope_kind, scope_value, context) in &group.scopes {
                let context_suffix = if context.is_empty() {
                    String::new()
                } else {
                    format!(
                        "  {dim}(context: {ctx}){reset}",
                        dim = p.context,
                        ctx = context,
                        reset = p.reset,
                    )
                };
                out.push_str(&format!(
                    "      {scope}{scope_kind}{reset} = {scope}{scope_value}{reset}{context_suffix}\n",
                    scope = p.scope,
                    reset = p.reset,
                ));
            }
            for item in group.owns {
                out.push_str(&format!(
                    "      {bullet}·{reset} {item}\n",
                    bullet = p.bullet,
                    reset = p.reset,
                    item = item,
                ));
            }
        }
        if !wrote_claim && descriptor.claims.is_empty() && query.scope_kind.is_none() {
            out.push_str(&format!(
                "\n  {dim}owns no protected semantics{reset}\n",
                dim = p.dim,
                reset = p.reset,
            ));
        }
        if query.scope_kind.is_none() && query.scope_value.is_none() && !descriptor.notes.is_empty()
        {
            out.push('\n');
            for note in &descriptor.notes {
                out.push_str(&format!(
                    "  {label}note{reset} {dim}{claim}:{reset} {text}\n",
                    label = p.note_label,
                    reset = p.reset,
                    dim = p.dim,
                    claim = note.claim,
                    text = note.text,
                ));
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
