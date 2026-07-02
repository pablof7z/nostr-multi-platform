//! Reusable feed open/close/reopen mechanics for UniFFI facades.
//!
//! These are the stateless bridge mechanics behind a facade's
//! `open_feed`/`close_feed` surface. They wrap the native-runtime composition
//! seam (`NmpApp::open_feed` / `close_feed`) and the canonical primary-kind
//! validator (`decode_and_validate_feed_params`), so an app-owned facade does not
//! copy that open/validate/teardown policy or choose a compiler.
//!
//! "Session" is internal runtime bookkeeping (#2508), not public vocabulary
//! (#2783) — every name here spells the app-facing "feed" / "handle" vocabulary
//! instead.
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
    decode_and_validate_feed_params, FeedHandle, FeedLoadStatus, FeedSessionId, NmpApp,
    ProjectionKey,
};

/// Outcome of opening (or reopening) a feed.
///
/// `projection_key` is the NMPU snapshot key the host subscribes to for feed
/// frames; the full opened-feed value is passed back to
/// [`load_older_feed`], [`close_feed`], and
/// [`reopen_feed`] for lifecycle commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedFeed {
    pub projection_key: String,
    pub handle_id: u64,
}

impl OpenedFeed {
    fn runtime_handle(&self) -> Option<FeedHandle> {
        Some(FeedHandle {
            projection_key: ProjectionKey::app_owned(self.projection_key.clone()).ok()?,
            session_id: FeedSessionId(self.handle_id),
        })
    }
}

/// Why a feed open failed.
///
/// Facades map these onto their facade-local UniFFI error namespace (for
/// example `InvalidInput` / `FeedOpenFailed` variants in the app facade).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedError {
    /// `params_json` is not valid JSON, or the `FeedParams` primary kinds fail
    /// validation (wrapper/delete/empty primary kind).
    InvalidParams,
    /// The runtime failed to register the feed (unsupported scope or
    /// poisoned registry).
    OpenFailed,
}

/// Decode + validate + open/register a feed from JSON params.
///
/// Uses `NmpApp::open_feed`, so the canonical compiler stays below the app facade
/// boundary. Fail-closed (D6): all failures are typed [`FeedError`]; never
/// panics.
///
/// # Errors
///
/// * [`FeedError::InvalidParams`] — invalid JSON or invalid primary kinds.
/// * [`FeedError::OpenFailed`] — the runtime could not register the feed.
pub fn open_feed(app: &NmpApp, params_json: &str) -> Result<OpenedFeed, FeedError> {
    let (params, _acquisition_kinds) =
        decode_and_validate_feed_params(params_json).map_err(|_| FeedError::InvalidParams)?;

    app.open_feed(&params)
        .map(|handle| OpenedFeed {
            projection_key: handle.projection_key.into_string(),
            handle_id: handle.session_id.0,
        })
        .map_err(|_| FeedError::OpenFailed)
}

/// Page a feed opened by [`open_feed`], addressed by its full opened-feed
/// handle.
///
/// Idempotent/fail-closed (D6): malformed, stale, unknown, or mismatched
/// handles are silent no-ops returning `false`.
#[must_use]
pub fn load_older_feed(app: &NmpApp, opened: &OpenedFeed) -> bool {
    load_older_feed_status(app, opened).changed
}

/// Page a feed and return the Rust-owned load stop reason.
#[must_use]
pub fn load_older_feed_status(app: &NmpApp, opened: &OpenedFeed) -> FeedLoadStatus {
    opened
        .runtime_handle()
        .map(|handle| app.load_older_feed_status(&handle))
        .unwrap_or_else(FeedLoadStatus::session_unavailable)
}

/// Tear down a feed opened by [`open_feed`], addressed by its full opened-feed
/// handle.
///
/// Idempotent (D6): closing an already-closed or unknown feed is a silent
/// no-op returning `false`. D8: the feed's resources are released
/// immediately and its registry entry is removed, so a subsequent close of the
/// same handle is always a no-op.
#[must_use]
pub fn close_feed(app: &NmpApp, opened: &OpenedFeed) -> bool {
    opened
        .runtime_handle()
        .is_some_and(|handle| app.close_feed(&handle))
}

/// Reopen a feed against the CURRENT runtime state, retaining the same
/// declaration.
///
/// This is the sanctioned "tear down + reopen" mechanic for feeds an
/// app-owned facade must rebuild after a perspective change — for example a
/// feed pinned to a specific account that must be recompiled when the
/// **active account** changes. It closes `old_opened` (idempotent — a stale,
/// already-closed, or mismatched handle is harmless) and opens a FRESH feed
/// from `params_json`, returning the new [`OpenedFeed`] (a new `handle_id`; the
/// `projection_key` is the same when the declaration's key is unchanged).
///
/// # When NOT to reopen
///
/// Account-reactive feeds (`FeedScope::ActiveUserFollows` and friends) re-seed
/// **in place** on an active-account change — the native runtime's
/// `register_identity_change_observer` wiring clears and repopulates them
/// without a reopen. Do not reopen those; reopening them needlessly drops and
/// rebuilds a live feed. Reopen is for declarations whose compiled shape is
/// pinned to the prior account and cannot re-seed reactively.
///
/// Fail-closed (D6): on open failure nothing is left registered (the old
/// feed is already closed and the new open failed closed before
/// registering), and the error is returned so the caller knows the reopen did
/// not produce a live feed.
///
/// # Errors
///
/// Same as [`open_feed`].
pub fn reopen_feed(
    app: &NmpApp,
    old_opened: &OpenedFeed,
    params_json: &str,
) -> Result<OpenedFeed, FeedError> {
    let _ = close_feed(app, old_opened);
    open_feed(app, params_json)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACTIVE_FOLLOWS_KIND1: &str = r#"{
        "primary_kinds": [1],
        "source": "ActiveUserFollows",
        "admission": "All",
        "order": "NewestByFeedPosition",
        "window": {"initial_limit": 50},
        "key": "app.feed.support.test",
        "item_projection": "FeedRows"
    }"#;

    #[test]
    fn open_rejects_malformed_json() {
        let app = nmp_native_runtime::new_app();
        assert_eq!(open_feed(&app, "{not json}"), Err(FeedError::InvalidParams));
    }

    #[test]
    fn open_rejects_invalid_primary_kind() {
        let app = nmp_native_runtime::new_app();
        // kind:6 is a repost wrapper; invalid as a primary kind.
        let json = r#"{
            "primary_kinds": [6],
            "source": "ActiveUserFollows",
            "admission": "All",
            "order": "NewestByFeedPosition",
            "window": {"initial_limit": 50},
            "key": "app.feed.support.invalid",
            "item_projection": "FeedRows"
        }"#;
        assert_eq!(open_feed(&app, json), Err(FeedError::InvalidParams));
    }

    #[test]
    fn open_then_close_then_close_is_idempotent() {
        let app = nmp_native_runtime::new_app();
        let Ok(opened) = open_feed(&app, ACTIVE_FOLLOWS_KIND1) else {
            assert!(
                false,
                "open must succeed for valid ActiveUserFollows/kind:1"
            );
            return;
        };
        assert!(!opened.projection_key.is_empty());
        assert_ne!(opened.handle_id, 0);

        assert!(
            close_feed(&app, &opened),
            "first close tears down the live feed"
        );
        assert!(!close_feed(&app, &opened), "second close is a no-op (D6)");
    }

    #[test]
    fn close_unknown_handle_is_noop() {
        let app = nmp_native_runtime::new_app();
        let unknown = OpenedFeed {
            projection_key: "app.feed.support.unknown".to_string(),
            handle_id: 99_999,
        };
        assert!(!close_feed(&app, &unknown));
    }

    #[test]
    fn load_status_unknown_handle_is_typed() {
        let app = nmp_native_runtime::new_app();
        let unknown = OpenedFeed {
            projection_key: "app.feed.support.unknown".to_string(),
            handle_id: 99_999,
        };
        let status = load_older_feed_status(&app, &unknown);
        assert!(!status.changed);
        assert_eq!(
            status.reason,
            nmp_native_runtime::FeedLoadStopReason::SessionUnavailable
        );
    }
}
