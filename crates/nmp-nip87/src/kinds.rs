//! Kind constants for NIP-87 (ecash mint discoverability).
//!
//! The kind *integers* are the workspace canon in `nmp-kinds` (the zero-dep
//! Layer-0 registry). Re-export the canonical names directly so all callers use
//! the single source of truth without per-crate aliases.

pub use nmp_kinds::{KIND_MINT_ANNOUNCE, KIND_MINT_RECOMMEND};
