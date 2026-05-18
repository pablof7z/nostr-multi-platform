//! `nmp-marmot` — Marmot Protocol (MLS-over-Nostr) NMP crate.
//!
//! SCAFFOLD: implementation pending. Adapter only — does NOT re-implement MLS.
//! Wraps `mdk-core` 0.8.0 + `mdk-sqlite-storage`. Spec: docs/plan/marmot-mls.md.
//!
//! Kernel-boundary exit gate: `nmp-core` gains zero MLS types; this crate is
//! the sole importer of `mdk-core`/`openmls`.

pub mod action;
pub mod domain;
pub mod view;

/// Register every `nmp-marmot` module into the kernel registry.
/// Filled in by the marmot impl agent (wave 2).
pub fn register(_registry: &mut nmp_core::substrate::ModuleRegistry) {}
