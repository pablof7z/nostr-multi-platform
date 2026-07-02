//! Feed viewport-command + session-lifecycle UniFFI surface — M14-C5.
//!
//! ## C-ABI parity
//!
//! | UniFFI method      | C-ABI counterpart                           |
//! |--------------------|---------------------------------------------|
//! | `load_older_feed`  | native feed viewport command, now handle-owned |
//!
//! `open_feed_json` and `close_feed_session` are NEW UniFFI-only surface: the
//! C-ABI retired its public open/close feed symbols before M14; the Rust-native
//! composition seam (`NmpApp::open_feed`) remains and is exposed here without
//! compiler selection at the UniFFI boundary.
//!
//! ## Handle lifecycle
//!
//! `open_feed_json` returns a [`FeedSessionHandle`] containing the projection
//! key (the NMPU snapshot key the host reads feed frames under) and a `u64`
//! session id. Pass that handle to `load_older_feed` and `close_feed_session`.
//! Teardown is idempotent: closing an already-closed, unknown, or mismatched
//! handle is a silent no-op (D6). The projection key is separate from the
//! session id so the host can subscribe to NMPU updates before calling
//! `close_feed_session`.
//!
//! ## Compiler boundary
//!
//! `open_feed_json` delegates to `nmp-uniffi-support`, which calls
//! `NmpApp::open_feed`. The canonical native feed compiler stays below that
//! runtime method; UniFFI callers cannot choose a compiler.
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
    load_older_feed_session as support_load_older_feed_session,
    open_feed_session as support_open_feed_session, FeedSessionError,
};

use crate::stateless::NmpError;
use crate::NmpApp;

// ── Shared UniFFI record ───────────────────────────────────────────────────────

/// Opaque handle for a feed session opened via `open_feed_json`.
///
/// `projection_key` — the NMPU snapshot key (e.g. `"microblog.timeline.home"`) the host
///   subscribes to for feed-frame updates.
/// `session_id` — the numeric session id. The handle is only valid when this id
///   still resolves to `projection_key`.
#[derive(uniffi::Record, Debug, Clone)]
pub struct FeedSessionHandle {
    pub projection_key: String,
    pub session_id: u64,
}

fn opened_from_handle(handle: &FeedSessionHandle) -> nmp_uniffi_support::OpenedFeed {
    nmp_uniffi_support::OpenedFeed {
        projection_key: handle.projection_key.clone(),
        session_id: handle.session_id,
    }
}

// ── NmpApp methods ────────────────────────────────────────────────────────────

#[uniffi::export]
impl NmpApp {
    /// Advance the feed's viewport to the next older page.
    ///
    /// Uses the full handle returned by `open_feed_json`; a raw projection key
    /// or raw session id is not sufficient to page a feed. Returns `true` when
    /// the viewport cursor actually changed; `false` for an unknown, closed, or
    /// mismatched handle, or when already at the oldest page (D6: always
    /// succeeds, never panics).
    pub fn load_older_feed(&self, handle: FeedSessionHandle) -> bool {
        support_load_older_feed_session(&self.inner, &opened_from_handle(&handle))
    }

    /// Open a new feed session from a JSON-encoded `FeedParams` declaration.
    ///
    /// Parses and validates the declaration, then opens the session through
    /// `NmpApp::open_feed` using the canonical native compiler below the facade
    /// boundary.
    /// Returns a [`FeedSessionHandle`] with the projection key and session id.
    ///
    /// D6: all failures are typed `NmpError` values — never panics.
    ///
    /// # Errors
    ///
    /// * `NmpError::InvalidInput` — `params_json` is not valid JSON or the
    ///   `FeedParams` primary kinds fail validation (e.g. a wrapper kind used as
    ///   a primary kind, or an empty primary-kinds list).
    /// * `NmpError::FeedOpenFailed` — the runtime failed to register the session
    ///   (e.g. an unsupported scope or poisoned registry).
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
    /// the registry. Returns `true` when a live matching session was torn down;
    /// `false` when the handle is unknown, mismatched, or already closed
    /// (idempotent — D6).
    ///
    /// D8: the session's resources are released immediately; the registry entry
    /// is removed so a subsequent close of the same handle is always a no-op.
    pub fn close_feed_session(&self, handle: FeedSessionHandle) -> bool {
        support_close_feed_session(&self.inner, &opened_from_handle(&handle))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Parity: load_older_feed ───────────────────────────────────────────

    /// Calling with an unknown handle must be a silent no-op returning `false` (D6).
    #[test]
    fn parity_load_older_feed_unknown_handle_is_noop() {
        let app = crate::NmpApp::new();
        let handle = FeedSessionHandle {
            projection_key: "app.feed.nonexistent".to_string(),
            session_id: 99_999,
        };
        let result = app.load_older_feed(handle);
        assert!(!result, "unknown feed handle must return false");
    }

    /// An invalid projection key inside the handle must be a silent no-op (D6).
    #[test]
    fn parity_load_older_feed_invalid_handle_is_noop() {
        let app = crate::NmpApp::new();
        let handle = FeedSessionHandle {
            projection_key: String::new(),
            session_id: 1,
        };
        let result = app.load_older_feed(handle);
        assert!(!result, "invalid feed handle must return false");
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
            "source": "ActiveUserFollows",
            "admission": "All",
            "order": "NewestByFeedPosition",
            "window": {"initial_limit": 50},
            "key": "app.feed.test.invalid",
            "item_projection": "FeedRows"
        }"#;
        let result = app.open_feed_json(params_json.to_string());
        assert!(
            matches!(result, Err(NmpError::InvalidInput)),
            "invalid primary kind must return InvalidInput"
        );
    }

    /// `open_feed_json` with valid kind:1 (note) + ActiveUserFollows scope must
    /// succeed before the runtime is started (the runtime registers interests
    /// that drain silently). The returned handle must have a non-empty projection
    /// key and a non-zero session id.
    #[test]
    fn parity_open_feed_json_valid_params_returns_handle() {
        let app = crate::NmpApp::new();
        // FeedParams JSON: field names match the struct (source, not scope;
        // FeedAdmission::All, not "Default"; FeedWindowPolicy fields such as
        // initial_limit, not a generic limit).
        let params_json = r#"{
            "primary_kinds": [1],
            "source": "ActiveUserFollows",
            "admission": "All",
            "order": "NewestByFeedPosition",
            "window": {"initial_limit": 50},
            "key": "app.feed.test",
            "item_projection": "FeedRows"
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
            "source": "ActiveUserFollows",
            "admission": "All",
            "order": "NewestByFeedPosition",
            "window": {"initial_limit": 50},
            "key": "app.feed.test.teardown",
            "item_projection": "FeedRows"
        }"#;
        let handle = app
            .open_feed_json(params_json.to_string())
            .expect("open must succeed");

        // First close — session is live.
        let torn_down = app.close_feed_session(handle.clone());
        assert!(torn_down, "first close must return true (session was live)");

        // Second close — session is already closed.
        let torn_down_again = app.close_feed_session(handle);
        assert!(
            !torn_down_again,
            "second close must return false (idempotent D6)"
        );
    }

    /// Closing an unknown handle (never opened) must be a silent no-op
    /// returning `false` (D6).
    #[test]
    fn teardown_close_unknown_handle_is_noop() {
        let app = crate::NmpApp::new();
        let result = app.close_feed_session(FeedSessionHandle {
            projection_key: "app.feed.unknown".to_string(),
            session_id: 99_999,
        });
        assert!(!result, "unknown handle must return false (D6)");
    }

    #[test]
    fn teardown_mismatched_handle_does_not_close_live_session() {
        let app = crate::NmpApp::new();
        let params_json = r#"{
            "primary_kinds": [1],
            "source": "ActiveUserFollows",
            "admission": "All",
            "order": "NewestByFeedPosition",
            "window": {"initial_limit": 50},
            "key": "app.feed.test.mismatch",
            "item_projection": "FeedRows"
        }"#;
        let handle = app
            .open_feed_json(params_json.to_string())
            .expect("open must succeed");
        let forged = FeedSessionHandle {
            projection_key: "app.feed.test.other".to_string(),
            session_id: handle.session_id,
        };

        assert!(
            !app.close_feed_session(forged),
            "mismatched handle must not close live session"
        );
        assert!(
            app.close_feed_session(handle),
            "real handle remains live and closes"
        );
    }
}
