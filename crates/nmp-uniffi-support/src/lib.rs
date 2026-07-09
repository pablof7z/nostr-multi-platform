//! Shared Rust-side mechanics for UniFFI facade contracts.
//!
//! This crate intentionally does **not** call `uniffi::setup_scaffolding!()`.
//! A native app links exactly one UniFFI cdylib, and that owning facade crate
//! calls `setup_scaffolding!()` once. Exported UniFFI records and callback
//! traits must therefore live in the owning facade crate's namespace; this
//! crate only shares the panic containment, quiescence, dispatch, and clamp
//! mechanics behind those facade-local types.
//!
//! # Safe runtime ownership (no raw `*mut NmpApp`)
//!
//! Every helper here takes the runtime by shared reference (`&NmpApp`) and
//! delivers callbacks through `Arc`-held sinks. None of them capture, store, or
//! return a raw `*mut NmpApp`. A UniFFI facade owns its
//! `nmp_native_runtime::NmpApp` **by value** inside its own `Arc<Facade>`
//! UniFFI object and passes `&self.inner` at every call, so there is no
//! sanctioned `*mut`/`unsafe` runtime handle for an app facade to capture. The
//! legacy `*mut NmpApp` address-capture pattern belonged to the deleted C-ABI
//! builder lane; the UniFFI-facade ownership model eliminates it structurally,
//! mirroring how the native runtime's own account-change wiring captures
//! granular `Arc` handles rather than the whole-app pointer. This is why the
//! crate adds no "owned runtime handle" helper: the right answer is the borrow
//! + `Arc`-sink shape used throughout.
//!
//! # Module map
//!
//! - [`dispatch`]: `DispatchOutcome` + `dispatch_action`/`dispatch_action_vec`.
//! - [`callbacks`]: update/capability/action-result/lifecycle Arc-sink +
//!   panic-containment mechanics.
//! - [`runtime_clamp`]: shared `visible_limit`/`emit_hz` clamp contract +
//!   `start_runtime`/`configure_runtime`.
//! - [`profile_key_guard`]: `is_hex_pubkey` facade input guard.
//! - [`account`]/[`sessions`]/[`composite_sessions`]: stateful-flow helpers
//!   (#2516/#3086) — feed-session open/close/reopen and active-account-change
//!   observation.
//! - [`keyed_read_collection`]: `KeyedReadCollection` constructors (#3115) —
//!   one per host open/close flavor.
//! - [`ownership`]: compiled ownership descriptor for crate-ownership reports.

/// Shared Arc-sink + panic-containment mechanics for UniFFI callback shapes.
pub mod callbacks;
/// Action-dispatch mechanics (`DispatchOutcome`, `dispatch_action`).
pub mod dispatch;
/// Shared facade input guard for profile-ref keys.
pub mod profile_key_guard;
/// Shared visible-limit/emit-hz clamp contract + runtime start/configure.
pub mod runtime_clamp;

#[cfg(test)]
mod facade_flow_tests;

pub use callbacks::{
    capability_handler_from_sink, clear_action_result_observer, dispatch_capability_json,
    lifecycle_observer_from_sink, register_action_result_observer, set_capability_callback,
    set_lifecycle_callback, set_update_sink, update_listener_from_sink,
};
pub use dispatch::{dispatch_action, dispatch_action_vec, DispatchOutcome};
pub use profile_key_guard::is_hex_pubkey;
pub use runtime_clamp::{clamp_emit_hz, clamp_visible, configure_runtime, start_runtime};

// ── Stateful-flow helpers (#2516) ─────────────────────────────────────────────
// Feed-session open/close/reopen mechanics and active-account-change
// observation, for app-owned facades with app-specific account-scoped sessions.

/// Active-account-change observation (shared Arc-sink + panic containment over
/// `NmpApp::register_identity_change_observer`).
pub mod account;
/// Composite multi-lane feed open mechanics over
/// `NmpApp::open_composite_feed` (#3086). Feature-gated: see
/// `composite_sessions`'s module doc.
#[cfg(feature = "composite-feed")]
pub mod composite_sessions;
/// Feed open/close/reopen mechanics over `NmpApp::open_feed`/`close_feed`.
pub mod sessions;

pub use account::{
    account_change_observer_from_sink, register_account_change_sink, unregister_account_change_sink,
};
#[cfg(feature = "composite-feed")]
pub use composite_sessions::open_composite_feed;
pub use sessions::{
    close_feed, load_older_feed, load_older_feed_status, open_feed, reopen_feed, FeedError,
    OpenedFeed,
};

/// Facade-composable constructors for `nmp_read_session::KeyedReadCollection`
/// against a live `NmpApp` (#3115): one per host open/close flavor
/// (read-session-backed, observed-projection-backed).
pub mod keyed_read_collection;
pub use keyed_read_collection::{
    keyed_observed_projection_collection, keyed_read_session_collection,
};

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
