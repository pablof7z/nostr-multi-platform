//! Canonical NMP component registry owner.
//!
//! This crate owns the upstream component registry manifests, source assets,
//! source lookup, and jsrepo export data model. `nmp-cli` owns command UX and
//! calls this crate for registry behavior; installed copies remain app-owned.

mod builtin_files;
pub mod export;
pub mod manifest;
mod ownership;
pub mod registry;

pub use registry::{Registry, RegistryComponent, RegistryFile};
