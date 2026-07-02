//! Session-lifecycle UniFFI surface (#2125).
//!
//! `nmp-uniffi` is the sole native binding surface for feed-viewport commands,
//! URI routing, and NIP-50 search sessions (M14 complete; the legacy
//! `nmp-ffi` C-ABI crate has been deleted). Each sub-module adds a
//! `#[uniffi::export] impl NmpApp` block exposing typed methods.
//!
//! ## Module layout
//!
//! | Module   | UniFFI methods                                                 |
//! |----------|------------------------------------------------------------------|
//! | `feed`   | `load_older_feed`, `open_feed_json`, `close_feed_session`      |
//! | `uri`    | `open_uri`                                                     |
//! | `search` | `search_open`, `search_close`, `search_snapshot`               |
//!
//! ## Feed session design note
//!
//! The C-ABI never had `open_feed`/`close_feed` symbols — those were retired
//! before M14 (confirmed by `feed_public_surface_retired.rs`). For UniFFI this
//! slice adds first-class feed-session management: `open_feed_json` accepts a
//! JSON-encoded `FeedParams` and delegates to `NmpApp::open_feed`, which applies
//! the canonical native feed compiler below the facade boundary.
//! `load_older_feed` and `close_feed_session` accept the `FeedSessionHandle`
//! returned by `open_feed_json`, so hosts do not page or tear down sessions by
//! replaying a raw projection key or raw session id.
//!
//! ## Snapshot return
//!
//! `search_snapshot` returns `Option<Vec<u8>>` directly (typed UniFFI return)
//! rather than the C-ABI's caller-provided output buffer + length integer. The
//! underlying `NmpApp::search_snapshot_bytes` already returns `Option<Vec<u8>>`,
//! so the UniFFI shape is the natural typed form of the same call.

pub mod feed;
pub mod search;
pub mod uri;

pub use feed::FeedSessionHandle;
