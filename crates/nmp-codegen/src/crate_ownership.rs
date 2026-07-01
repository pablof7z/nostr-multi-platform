//! Workspace ownership report + audit support for `nmp crate-ownership`.
//!
//! Descriptors live in each crate's Rust source via
//! `nmp_ownership::declare_crate_ownership!`. This module only discovers and
//! validates the active Cargo workspace; it is not a hand-maintained registry.

mod audit;
mod render;
mod workspace;

use serde::Serialize;
use std::path::PathBuf;

pub use render::{render_ownership_human, render_ownership_json, render_ownership_tsv};
pub use workspace::load_workspace_ownership;

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
