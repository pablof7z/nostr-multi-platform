//! Host-side runtime installer bridges.
//!
//! The canonical implementations now live in the protocol crates that own each
//! protocol. This module re-exports them so the existing call sites in
//! `nmp_defaults::runtimes::*` and `nmp_defaults::register_*` remain
//! unchanged during the transition.
//!
//! | Function                          | Canonical home         |
//! |-----------------------------------|------------------------|
//! | `register_dm_runtime`             | `nmp_nip17::installer` |
//! | `register_mute_runtime`           | `nmp_nip51`            |
//! | `register_bookmark_runtime`       | `nmp_nip51`            |
//! | `register_bookmark_set_runtime`   | `nmp_nip51`            |
//! | `register_web_bookmark_runtime`   | `nmp_nip51`            |
//! | `register_comment_runtime`        | `nmp_nip22`            |
//! | `register_search_relay_runtime`   | `nmp_nip51`            |
//! | `register_search_relay_runtime_with` | `nmp_nip51`         |

pub use nmp_nip17::register_dm_runtime;

mod mute_runtime;
pub use mute_runtime::register_mute_runtime;

mod bookmarks_runtime;
pub use bookmarks_runtime::{
    register_bookmark_runtime, register_bookmark_set_runtime, register_web_bookmark_runtime,
};

mod comments_runtime;
pub use comments_runtime::register_comment_runtime;

mod search_relay_runtime;
pub use search_relay_runtime::{register_search_relay_runtime, register_search_relay_runtime_with};

// Mute-list active observed-projection reconciler tests.
#[cfg(test)]
#[path = "runtimes_mute_tests.rs"]
mod mute_tests;
