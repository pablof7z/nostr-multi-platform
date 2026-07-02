//! Reusable feed-session open/close/reopen mechanics for UniFFI facades.
//!
//! These are the stateless bridge mechanics behind a facade's
//! `open_feed`/`close_feed` surface. They wrap the native-runtime composition
//! seam (`NmpApp::open_feed` / `close_feed`) and the canonical primary-kind
//! validator (`decode_and_validate_feed_params`), so an app-owned facade does not
//! copy that open/validate/session-teardown policy or choose a compiler.
//!
//! # Safe ownership (no raw runtime pointer)
//!
//! Every helper takes the runtime by shared reference (`&NmpApp`). None of them
//! capture, store, or hand out a `*mut NmpApp`. The facade already owns its
//! `nmp_native_runtime::NmpApp` (by value inside its `Arc<Facade>` UniFFI
//! object) and passes `&self.inner` at each call. There is therefore no
//! sanctioned `*mut`/`unsafe` runtime handle to capture — see the crate-level
//! note in `lib.rs`.

use nmp_native_runtime::{
    decode_and_validate_feed_params, FeedHandle, FeedSessionId, NmpApp, ProjectionKey,
};

/// Outcome of opening (or reopening) a feed session.
///
/// `projection_key` is the NMPU snapshot key the host subscribes to for feed
/// frames; `session_id` is the numeric id passed to [`close_feed_session`] /
/// [`reopen_feed_session`] for teardown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedFeed {
    pub projection_key: String,
    pub session_id: u64,
}

/// Why a feed-session open failed.
///
/// Facades map these onto their facade-local UniFFI error namespace (e.g.
/// `nmp-uniffi`'s `NmpError::InvalidInput` / `NmpError::FeedOpenFailed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedSessionError {
    /// `params_json` is not valid JSON, or the `FeedParams` primary kinds fail
    /// validation (wrapper/delete/empty primary kind).
    InvalidParams,
    /// The runtime failed to register the session (unsupported scope or
    /// poisoned registry).
    OpenFailed,
}

/// Decode + validate + open/register a feed session from JSON params.
///
/// Uses `NmpApp::open_feed`, so the canonical compiler stays below the app facade
/// boundary. Fail-closed (D6): all failures are typed [`FeedSessionError`]; never
/// panics.
///
/// # Errors
///
/// * [`FeedSessionError::InvalidParams`] — invalid JSON or invalid primary kinds.
/// * [`FeedSessionError::OpenFailed`] — the runtime could not register the session.
pub fn open_feed_session(app: &NmpApp, params_json: &str) -> Result<OpenedFeed, FeedSessionError> {
    let (params, _acquisition_kinds) = decode_and_validate_feed_params(params_json)
        .map_err(|_| FeedSessionError::InvalidParams)?;

    app.open_feed(&params)
        .map(|handle| OpenedFeed {
            projection_key: handle.projection_key.into_string(),
            session_id: handle.session_id.0,
        })
        .map_err(|_| FeedSessionError::OpenFailed)
}

/// Tear down a feed session opened by [`open_feed_session`], addressed by its
/// numeric session id.
///
/// Idempotent (D6): closing an already-closed or unknown session is a silent
/// no-op returning `false`. D8: the session's resources are released
/// immediately and its registry entry is removed, so a subsequent close of the
/// same id is always a no-op.
#[must_use]
pub fn close_feed_session(app: &NmpApp, session_id: u64) -> bool {
    let handle = FeedHandle {
        // Only `session_id` is read by `close_feed`; the projection key is not
        // re-derived (close addresses the recorded teardown by id, not a filter).
        projection_key: ProjectionKey::app_owned("app.feed.close.placeholder").unwrap(),
        session_id: FeedSessionId(session_id),
    };
    app.close_feed(&handle)
}

/// Reopen a feed session against the CURRENT runtime state, retaining the same
/// declaration.
///
/// This is the sanctioned "tear down + reopen" mechanic for sessions an
/// app-owned facade must rebuild after a perspective change — for example a
/// session pinned to a specific account that must be recompiled when the
/// **active account** changes. It closes `old_session_id` (idempotent — a stale
/// or already-closed id is harmless) and opens a FRESH session from
/// `params_json`, returning the new [`OpenedFeed`] (a new `session_id`; the
/// `projection_key` is the same when the declaration's projection is unchanged).
///
/// # When NOT to reopen
///
/// Account-reactive feeds (`FeedScope::ActiveUserFollows` and friends) re-seed
/// **in place** on an active-account change — the native runtime's
/// `register_identity_change_observer` wiring clears and repopulates them
/// without a reopen. Do not reopen those; reopening them needlessly drops and
/// rebuilds a live session. Reopen is for declarations whose compiled shape is
/// pinned to the prior account and cannot re-seed reactively.
///
/// Fail-closed (D6): on open failure nothing is left registered (the old
/// session is already closed and the new open failed closed before
/// registering), and the error is returned so the caller knows the reopen did
/// not produce a live session.
///
/// # Errors
///
/// Same as [`open_feed_session`].
pub fn reopen_feed_session(
    app: &NmpApp,
    old_session_id: u64,
    params_json: &str,
) -> Result<OpenedFeed, FeedSessionError> {
    let _ = close_feed_session(app, old_session_id);
    open_feed_session(app, params_json)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACTIVE_FOLLOWS_KIND1: &str = r#"{
        "primary_kinds": [1],
        "acquisition": "ActiveUserFollows",
        "admission": "All",
        "ranking": "ChronologicalDesc",
        "window": {"initial_limit": 50},
        "projection": "app.feed.support.test"
    }"#;

    #[test]
    fn open_rejects_malformed_json() {
        let app = nmp_native_runtime::new_app();
        assert_eq!(
            open_feed_session(&app, "{not json}"),
            Err(FeedSessionError::InvalidParams)
        );
    }

    #[test]
    fn open_rejects_invalid_primary_kind() {
        let app = nmp_native_runtime::new_app();
        // kind:6 is a repost wrapper; invalid as a primary kind.
        let json = r#"{
            "primary_kinds": [6],
            "acquisition": "ActiveUserFollows",
            "admission": "All",
            "ranking": "ChronologicalDesc",
            "window": {"initial_limit": 50},
            "projection": "app.feed.support.invalid"
        }"#;
        assert_eq!(
            open_feed_session(&app, json),
            Err(FeedSessionError::InvalidParams)
        );
    }

    #[test]
    fn open_then_close_then_close_is_idempotent() {
        let app = nmp_native_runtime::new_app();
        let Ok(opened) = open_feed_session(&app, ACTIVE_FOLLOWS_KIND1) else {
            assert!(
                false,
                "open must succeed for valid ActiveUserFollows/kind:1"
            );
            return;
        };
        assert!(!opened.projection_key.is_empty());
        assert_ne!(opened.session_id, 0);

        assert!(
            close_feed_session(&app, opened.session_id),
            "first close tears down live session"
        );
        assert!(
            !close_feed_session(&app, opened.session_id),
            "second close is a no-op (D6)"
        );
    }

    #[test]
    fn close_unknown_session_is_noop() {
        let app = nmp_native_runtime::new_app();
        assert!(!close_feed_session(&app, 99_999));
    }
}
