//! Generic feed viewport parameter validation.
//!
//! App crates register reusable feed controllers on an `NmpApp`; native shells
//! report viewport intent by key. The controller and page policy live in NMP.

/// Re-export the canonical typed feed-session declaration model (#1740).
///
/// The protocol-agnostic param model lives in `nmp-feed` (single source). This
/// FFI module re-exports the param types while the runtime-independent
/// `nmp-feed-session` compiler owns primary-kind validation and acquisition
/// derivation. Platform runtimes must not depend on NIP-18 directly just to
/// open generic feeds.
pub use nmp_feed::{
    feed, source, CustomAdmissionDef, CustomAdmissionId, CustomOrderDef, CustomOrderId,
    CustomSourceDef, CustomSourceId, FeedAdmission, FeedHandle, FeedItemProjection, FeedKey,
    FeedLoadStatus, FeedLoadStopReason, FeedOrder, FeedParams, FeedScope, FeedSessionId, FeedShape,
    FeedSourceExpr, FeedSpec, FeedSpecError, FeedWindowPolicy, ProjectionKey,
};
pub use nmp_feed_session::validate_feed_params;

use nmp_feed_session::PrimaryKindError;

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
    let acquisition_kinds =
        validate_feed_params(&params).map_err(FeedParamsDecodeError::InvalidParams)?;
    Ok((params, acquisition_kinds))
}

/// Typed error for FFI-boundary `FeedParams` decode + validation (D6 — no panic).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeedParamsDecodeError {
    /// The JSON payload did not parse into a `FeedParams`.
    MalformedJson,
    /// The decoded params failed validation (e.g. wrapper/delete primary kind).
    InvalidParams(PrimaryKindError),
}

#[cfg(test)]
mod feed_params_decode_tests {
    use super::*;

    fn params_json(primary_kinds: &str) -> String {
        format!(
            r#"{{
              "primary_kinds": {primary_kinds},
              "source": "ActiveUserFollows",
              "admission": "All",
              "order": "NewestByFeedPosition",
              "window": {{ "initial_limit": 80 }},
              "key": "app.feed.following",
              "item_projection": "FeedRows"
            }}"#
        )
    }

    #[test]
    fn valid_primary_kinds_decode_and_validate() {
        let (params, kinds) =
            decode_and_validate_feed_params(&params_json("[1]")).expect("[1] is valid");
        assert_eq!(params.primary_kinds, vec![1]);
        assert!(kinds.contains(&1) && kinds.contains(&6));
        assert_eq!(params.source, FeedSourceExpr::ActiveUserFollows);
    }

    #[test]
    fn wrapper_and_delete_primary_kinds_are_rejected_at_the_boundary() {
        assert_eq!(
            decode_and_validate_feed_params(&params_json("[1, 6]")),
            Err(FeedParamsDecodeError::InvalidParams(
                PrimaryKindError::RepostWrapper { kind: 6 }
            ))
        );
        assert_eq!(
            decode_and_validate_feed_params(&params_json("[1, 5]")),
            Err(FeedParamsDecodeError::InvalidParams(
                PrimaryKindError::DeleteKind
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
