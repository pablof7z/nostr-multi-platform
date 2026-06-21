//! Generic feed viewport FFI.
//!
//! App crates register reusable feed controllers on an `NmpApp`; native shells
//! report viewport intent by key. The controller and page policy live in NMP.

use std::ffi::c_char;

use crate::{app_ref, c_string_argument};

/// Re-export the canonical typed feed-session declaration model (#1740).
///
/// The model lives in `nmp-feed` (single source). This FFI module re-exports
/// the param types and the decode/validation surface so native shells can
/// decode a `FeedParams` JSON payload and run fail-closed primary-kind
/// validation **before** `open_feed` exists. Step 1 is types + decode +
/// validation only; the `open_feed` C-ABI dispatch lands in step 2.
///
/// `CustomPerspectiveId` and `validate_primary_kinds` are intentionally not
/// re-exported here: they have no external consumer yet and will be hoisted
/// through the facade when `open_feed` dispatch lands in step 2.
pub use nmp_feed::{
    FeedAdmission, FeedHandle, FeedParams, FeedParamsError, FeedRanking, FeedScope, FeedSessionId,
    FeedWindow, ProjectionKey, PubkeySetExpr,
};

/// Decode a `FeedParams` JSON payload and validate its primary kinds.
///
/// This is the boundary-safe entry the step-2 `open_feed` symbol will build on:
/// it parses untrusted JSON into the typed [`FeedParams`] and runs fail-closed
/// primary-kind validation (rejecting wrapper kinds 6/16 and delete kind 5). It
/// performs **no** dispatch — it neither opens a session nor mutates the app.
///
/// Returns the parsed params alongside the compiled acquisition kind set on
/// success, or a typed error on malformed JSON / invalid primary kinds.
pub fn decode_and_validate_feed_params(
    json: &str,
) -> Result<(FeedParams, std::collections::BTreeSet<u32>), FeedParamsDecodeError> {
    let params: FeedParams =
        serde_json::from_str(json).map_err(|_| FeedParamsDecodeError::MalformedJson)?;
    let acquisition_kinds = params
        .validate_primary_kinds()
        .map_err(FeedParamsDecodeError::InvalidParams)?;
    Ok((params, acquisition_kinds))
}

/// Typed error for FFI-boundary `FeedParams` decode + validation (D6 — no panic).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeedParamsDecodeError {
    /// The JSON payload did not parse into a `FeedParams`.
    MalformedJson,
    /// The decoded params failed validation (e.g. wrapper/delete primary kind).
    InvalidParams(FeedParamsError),
}

#[no_mangle]
pub extern "C" fn nmp_app_load_older_feed(app: *mut crate::NmpApp, key: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(key) = c_string_argument(key) else {
        return;
    };
    let _ = app.load_older_feed(&key);
}

#[cfg(test)]
mod feed_params_decode_tests {
    use super::*;

    fn params_json(primary_kinds: &str) -> String {
        format!(
            r#"{{
              "primary_kinds": {primary_kinds},
              "acquisition": "ActiveUserFollows",
              "admission": "All",
              "ranking": "ChronologicalDesc",
              "window": {{ "initial_limit": 80 }},
              "projection": "nmp.feed.home"
            }}"#
        )
    }

    #[test]
    fn valid_primary_kinds_decode_and_validate() {
        let (params, kinds) =
            decode_and_validate_feed_params(&params_json("[1]")).expect("[1] is valid");
        assert_eq!(params.primary_kinds, vec![1]);
        assert!(kinds.contains(&1) && kinds.contains(&6));
        assert_eq!(params.acquisition, PubkeySetExpr::ActiveUserFollows);
    }

    #[test]
    fn wrapper_and_delete_primary_kinds_are_rejected_at_the_boundary() {
        assert_eq!(
            decode_and_validate_feed_params(&params_json("[1, 6]")),
            Err(FeedParamsDecodeError::InvalidParams(
                FeedParamsError::RepostWrapperKind { kind: 6 }
            ))
        );
        assert_eq!(
            decode_and_validate_feed_params(&params_json("[1, 5]")),
            Err(FeedParamsDecodeError::InvalidParams(
                FeedParamsError::DeleteKind
            ))
        );
    }

    #[test]
    fn malformed_json_fails_closed() {
        assert_eq!(
            decode_and_validate_feed_params("{ not json"),
            Err(FeedParamsDecodeError::MalformedJson)
        );
    }
}
