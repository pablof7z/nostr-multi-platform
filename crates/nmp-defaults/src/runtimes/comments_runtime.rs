//! NIP-22 comment runtime — thin re-export bridge.
//!
//! The implementation now lives in `nmp_nip22::register_comment_runtime`.
//! This module re-exports it so the existing
//! `runtimes::register_comment_runtime` / `nmp_defaults::register_comment_runtime`
//! paths are unchanged during the transition.

pub use nmp_nip22::register_comment_runtime;
