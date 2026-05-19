//! NIP-01 kinds owned by this crate.
//!
//! Only kind 1 (short text note). Profile (kind 0) and contact list (kind 3)
//! are NIP-01 kinds too but currently live in `nmp-core`'s ingest path; their
//! extraction is a separate doctrine effort (kind-wrappers.md Phase 1) and
//! intentionally out of scope here.

/// NIP-01 short text note.
pub const KIND_SHORT_NOTE: u32 = 1;
