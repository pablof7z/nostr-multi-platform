//! NIP-51 bookmark-list runtime — thin re-export bridge.
//!
//! The implementation now lives in `nmp_nip51::{register_bookmark_runtime,
//! register_bookmark_set_runtime, register_web_bookmark_runtime}`.
//! This module re-exports them so the existing `runtimes::register_bookmark_*`
//! / `nmp_defaults::register_bookmark_*` paths are unchanged during the
//! transition.

pub use nmp_nip51::{
    register_bookmark_runtime, register_bookmark_set_runtime, register_web_bookmark_runtime,
};

// Co-located bookmark active observed-projection reconciler tests live in a
// sibling file to hold this module under the 300-LOC ceiling.
#[cfg(test)]
#[path = "runtimes_bookmarks_tests.rs"]
mod bookmarks_tests;
