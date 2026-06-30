//! NIP-51 mute-list runtime — thin re-export bridge.
//!
//! The implementation now lives in `nmp_nip51::register_mute_runtime`.
//! This module re-exports it so the existing
//! `runtimes::register_mute_runtime` / `nmp_defaults::register_mute_runtime`
//! paths are unchanged during the transition.

pub use nmp_nip51::register_mute_runtime;
