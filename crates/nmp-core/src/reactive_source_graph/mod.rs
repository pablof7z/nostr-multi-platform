//! Internal source dependency graph for Rust-owned reactive read sessions.
//!
//! This module is substrate only: no app nouns, no protocol nouns, no FFI
//! surface, no scheduler thread, and no async runtime. It gives higher-level
//! read/session code a deterministic way to turn typed source input changes into
//! derived values and coalesced internal effects.

mod graph;
mod graph_impl;
mod id;
#[cfg(test)]
mod tests;
mod value;

pub use graph::{
    GraphError, GraphRead, GraphTurn, NodeChange, ReactiveSourceGraph, SourceInputUpdate,
};
pub use id::{GraphScopeId, SourceNodeId, SourceNodeRevision};
