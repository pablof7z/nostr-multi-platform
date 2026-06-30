//! NIP-51 search-relay-list runtime — thin re-export bridge.
//!
//! The implementation now lives in
//! `nmp_nip51::{register_search_relay_runtime, register_search_relay_runtime_with}`.
//! This module re-exports them so the existing
//! `runtimes::register_search_relay_runtime` /
//! `nmp_defaults::register_search_relay_runtime` paths are unchanged during
//! the transition.

pub use nmp_nip51::{register_search_relay_runtime, register_search_relay_runtime_with};
