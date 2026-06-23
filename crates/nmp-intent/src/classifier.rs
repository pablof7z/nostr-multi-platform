//! The classification pipeline internals (issue #1804) — generic parsing pass
//! (raw input → `nmp_core::substrate::ResolvedInput`), the frozen-precedence
//! cascade, and the free-text → `nmp_nip50::SearchRequest` bridge.
//!
//! S1 fills these bodies; the module exists now so [`crate::classify`] and the
//! slices that depend on it have a stable home and the crate compiles.

// S1: generic parsing pass + precedence cascade live here.
