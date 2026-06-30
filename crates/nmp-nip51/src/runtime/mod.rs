//! NIP-51 runtime installers.
//!
//! These helpers wire NIP-51-owned projections, typed sidecars, action modules,
//! and active-account observed-projection reconcilers into a host. They are
//! intentionally per-feature APIs rather than an aggregate defaults bundle.

mod bookmarks;
mod mute;
mod search_relay;

pub use bookmarks::{
    register_bookmark_runtime, register_bookmark_set_runtime, register_web_bookmark_runtime,
};
pub use mute::register_mute_runtime;
pub use search_relay::register_search_relay_runtime_with_fallbacks;
