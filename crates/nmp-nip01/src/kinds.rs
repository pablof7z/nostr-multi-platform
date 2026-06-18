//! NIP-01 kinds owned by this crate.
//!
//! The kind *integer* itself is the workspace canon in `nmp-kinds` (re-exported
//! by `nmp-core::kinds`); this module re-exports it so NIP-01 code reads the
//! constant from one source of truth instead of re-declaring the literal `1`.
//! Profile (kind 0) and contact list (kind 3) are NIP-01 kinds too but
//! currently live in `nmp-core`'s ingest path; their extraction is a separate
//! doctrine effort (kind-wrappers.md Phase 1) and intentionally out of scope
//! here.

/// NIP-01 short text note (canonical source: `nmp-kinds::KIND_SHORT_TEXT_NOTE`).
pub use nmp_core::kinds::KIND_SHORT_TEXT_NOTE;
