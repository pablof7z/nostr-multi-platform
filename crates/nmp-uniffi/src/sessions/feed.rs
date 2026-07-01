//! Feed viewport-command + session-lifecycle UniFFI surface — M14-C5.
//!
//! ## C-ABI parity
//!
//! | UniFFI method      | C-ABI counterpart                           |
//! |--------------------|---------------------------------------------|
//! | `load_older_feed`  | native feed viewport command               |
//!
//! `open_feed_json` and `close_feed_session` are NEW UniFFI-only surface: the
//! C-ABI retired its public open/close feed symbols before M14; the Rust-native
//! composition seam (`NmpApp::open_feed`) remains and is exposed here using
//! `nmp_native_runtime::compile_feed_params` as the default compiler.
//!
//! ## Handle lifecycle
//!
//! `open_feed_json` returns a [`FeedSessionHandle`] containing the projection
//! key (the NMPU snapshot key the host reads feed frames under) and a `u64`
//! session id. Pass the `session_id` to `close_feed_session` to tear down.
//! Teardown is idempotent: closing an already-closed or unknown session is a
//! silent no-op (D6). The projection key is separate from the session id so the
//! host can subscribe to NMPU updates before calling `close_feed_session`.
//!
//! ## Compiler choice
//!
//! `open_feed_json` hard-wires `compile_feed_params` as the compiler. This is
//! the native-runtime feed compiler, not an app-specific override. A future
//! slice can expose a pluggable compiler seam if needed; for M14-C5 this is the
//! only wired path.
//!
//! ## Shared mechanic (#2516)
//!
//! The decode/validate/compile/open and idempotent-close mechanics are NOT
//! owned here — they are the reusable `nmp_uniffi_support::open_feed_session` /
//! `close_feed_session` helpers, so an app-owned UniFFI facade reuses the exact
//! same open/teardown policy. This crate's methods only adapt the facade-local
//! `FeedSessionHandle` record and `NmpError` namespace onto those helpers.

use nmp_uniffi_support::{
    close_feed_session as support_close_feed_session,
    open_feed_session as support_open_feed_session, FeedSessionError,
};

use crate::stateless::NmpError;
use crate::NmpApp;

// ── Shared UniFFI record ───────────────────────────────────────────────────────

/// Opaque handle for a feed session opened via `open_feed_json`.
///
/// `projection_key` — the NMPU snapshot key (e.g. `"app.feed.home"`) the host
///   subscribes to for feed-frame updates. Pass it to `load_older_feed` for
///   viewport paging commands.
/// `session_id` — the numeric session id; pass it to `close_feed_session` for
///   teardown.
#[derive(uniffi::Record, Debug, Clone)]
pub struct FeedSessionHandle {
    pub projection_key: String,
    pub session_id: u64,
}

// ── NmpApp methods ────────────────────────────────────────────────────────────

#[uniffi::export]
impl NmpApp {
    /// Advance the feed's viewport to the next older page.
    ///
    /// `key` is the projection key of the
    /// feed to page (the same string returned in `FeedSessionHandle.projection_key`
    /// or a well-known constant like `"app.feed.home"`). Returns `true` when the
    /// viewport cursor actually changed; `false` for an unknown key or when
    /// already at the oldest page (D6: always succeeds, never panics).
    pub fn load_older_feed(&self, key: String) -> bool {
        self.inner.load_older_feed(&key)
    }

    /// Open a new feed session from a JSON-encoded `FeedParams` declaration.
    ///
    /// Parses and validates the declaration, then compiles and registers the
    /// session using `compile_feed_params` (the composition-root default compiler).
    /// Returns a [`FeedSessionHandle`] with the projection key and session id.
    ///
    /// D6: all failures are typed `NmpError` values — never panics.
    ///
    /// # Errors
    ///
    /// * `NmpError::InvalidInput` — `params_json` is not valid JSON or the
    ///   `FeedParams` primary kinds fail validation (e.g. a wrapper kind used as
    ///   a primary kind, or an empty primary-kinds list).
    /// * `NmpError::FeedOpenFailed` — the compiler failed to register the
    ///   session (e.g. an unsupported scope or poisoned registry).
    pub fn open_feed_json(&self, params_json: String) -> Result<FeedSessionHandle, NmpError> {
        support_open_feed_session(&self.inner, &params_json)
            .map(|opened| FeedSessionHandle {
                projection_key: opened.projection_key,
                session_id: opened.session_id,
            })
            .map_err(|err| match err {
                FeedSessionError::InvalidParams => NmpError::InvalidInput,
                FeedSessionError::OpenFailed => NmpError::FeedOpenFailed,
            })
    }

    /// Close a feed session previously opened by `open_feed_json`.
    ///
    /// Tears down the observer, projection, pull-controller, and interests
    /// registered when the session was opened, then removes the session from
    /// the registry. Returns `true` when a live session was torn down; `false`
    /// when the `session_id` is unknown or already closed (idempotent — D6).
    ///
    /// D8: the session's resources are released immediately; the registry entry
    /// is removed so a subsequent close of the same id is always a no-op.
    pub fn close_feed_session(&self, session_id: u64) -> bool {
        support_close_feed_session(&self.inner, session_id)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Parity: load_older_feed ───────────────────────────────────────────

    /// Calling with an unknown key must be a silent no-op returning `false` (D6).
    #[test]
    fn parity_load_older_feed_unknown_key_is_noop() {
        let app = crate::NmpApp::new();
        let result = app.load_older_feed("nmp.feed.nonexistent".to_string());
        assert!(!result, "unknown feed key must return false");
    }

    /// An empty key must be a silent no-op (D6: invalid input).
    #[test]
    fn parity_load_older_feed_empty_key_is_noop() {
        let app = crate::NmpApp::new();
        let result = app.load_older_feed(String::new());
        assert!(!result, "empty feed key must return false");
    }

    // ── Parity: open_feed_json / close_feed_session ───────────────────────

    /// Malformed JSON must return `NmpError::InvalidInput`.
    #[test]
    fn parity_open_feed_json_malformed_json_returns_invalid_input() {
        let app = crate::NmpApp::new();
        let result = app.open_feed_json("{not valid json}".to_string());
        assert!(
            matches!(result, Err(NmpError::InvalidInput)),
            "malformed JSON must return InvalidInput"
        );
    }

    /// Empty string must return `NmpError::InvalidInput`.
    #[test]
    fn parity_open_feed_json_empty_string_returns_invalid_input() {
        let app = crate::NmpApp::new();
        let result = app.open_feed_json(String::new());
        assert!(
            matches!(result, Err(NmpError::InvalidInput)),
            "empty params_json must return InvalidInput"
        );
    }

    /// Invalid primary kinds (e.g. kind:6 — a repost wrapper) must return
    /// `NmpError::InvalidInput` at the validation gate.
    #[test]
    fn parity_open_feed_json_invalid_primary_kind_returns_invalid_input() {
        let app = crate::NmpApp::new();
        // kind:6 is a repost-wrapper kind; using it as a primary kind fails
        // the validator at the `validate_feed_params` gate in `decode_and_validate`.
        let params_json = r#"{
            "primary_kinds": [6],
            "acquisition": "ActiveUserFollows",
            "admission": "All",
            "ranking": "ChronologicalDesc",
            "window": {"initial_limit": 50},
            "projection": "app.feed.test.invalid"
        }"#;
        let result = app.open_feed_json(params_json.to_string());
        assert!(
            matches!(result, Err(NmpError::InvalidInput)),
            "invalid primary kind must return InvalidInput"
        );
    }

    /// `open_feed_json` with valid kind:1 (note) + ActiveUserFollows scope must
    /// succeed before the runtime is started (the compiler registers interests
    /// that drain silently). The returned handle must have a non-empty projection
    /// key and a non-zero session id.
    #[test]
    fn parity_open_feed_json_valid_params_returns_handle() {
        let app = crate::NmpApp::new();
        // FeedParams JSON: field names match the struct (acquisition, not scope;
        // FeedAdmission::All, not "Default"; FeedWindow.initial_limit, not limit).
        let params_json = r#"{
            "primary_kinds": [1],
            "acquisition": "ActiveUserFollows",
            "admission": "All",
            "ranking": "ChronologicalDesc",
            "window": {"initial_limit": 50},
            "projection": "app.feed.test"
        }"#;
        let result = app.open_feed_json(params_json.to_string());
        let handle = result.expect("valid ActiveUserFollows/kind:1 params must succeed");
        assert!(
            !handle.projection_key.is_empty(),
            "projection_key must be non-empty"
        );
        assert_ne!(handle.session_id, 0, "session_id must be non-zero");
    }

    // ── Teardown / idempotency ─────────────────────────────────────────────

    /// Open → close → close (idempotent): the second close must be a no-op
    /// returning `false` without panicking. This is the canonical teardown test.
    #[test]
    fn teardown_open_then_close_then_close_idempotent() {
        let app = crate::NmpApp::new();
        let params_json = r#"{
            "primary_kinds": [1],
            "acquisition": "ActiveUserFollows",
            "admission": "All",
            "ranking": "ChronologicalDesc",
            "window": {"initial_limit": 50},
            "projection": "app.feed.test.teardown"
        }"#;
        let handle = app
            .open_feed_json(params_json.to_string())
            .expect("open must succeed");

        // First close — session is live.
        let torn_down = app.close_feed_session(handle.session_id);
        assert!(torn_down, "first close must return true (session was live)");

        // Second close — session is already closed.
        let torn_down_again = app.close_feed_session(handle.session_id);
        assert!(
            !torn_down_again,
            "second close must return false (idempotent D6)"
        );
    }

    /// Closing an unknown session id (never opened) must be a silent no-op
    /// returning `false` (D6).
    #[test]
    fn teardown_close_unknown_session_id_is_noop() {
        let app = crate::NmpApp::new();
        let result = app.close_feed_session(99999);
        assert!(!result, "unknown session_id must return false (D6)");
    }
}
