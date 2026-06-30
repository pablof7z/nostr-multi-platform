//! App-default search-relay configuration — thin re-export bridge.
//!
//! The canonical implementations now live in `nmp_nip51::search_defaults`.
//! This module re-exports them so existing call sites at
//! `nmp_defaults::SearchDefaults` / `nmp_defaults::effective_search_relays`
//! remain unchanged during the transition.

pub use nmp_nip51::{effective_search_relays, SearchDefaults};
