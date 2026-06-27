//! Generic feed viewport parameter validation.
//!
//! App crates register reusable feed controllers on an `NmpApp`; native shells
//! report viewport intent by key. The controller and page policy live in NMP.

/// Re-export the canonical typed feed-session declaration model (#1740).
///
/// The protocol-agnostic param model lives in `nmp-feed` (single source). This
/// FFI module re-exports the param types and owns the primary-kind validation —
/// which IS protocol knowledge (it names the derived-acquisition wrapper/delete
/// kinds) and so lives in this composition layer, NOT in the generic
/// `nmp-feed` engine (D0). The validation transform itself is the single
/// canonical `nmp_nip18` transform; this layer only adds the empty-set guard.
pub use nmp_feed::{
    FeedAdmission, FeedHandle, FeedParams, FeedRanking, FeedRender, FeedScope, FeedSessionId,
    FeedWindow, ProjectionKey, PubkeySetExpr,
};

/// Typed error for a `FeedParams` declaration whose primary kinds are invalid
/// (D6 — no panic).
///
/// This is the single canonical primary-kind validation error
/// [`nmp_nip18::PrimaryKindError`], re-exported under a feed-facing alias.
/// Deciding that wrapper kinds (6/16) and the delete kind (5) are derived
/// acquisition rather than primary input is protocol knowledge, so the error and
/// the validator both live in the protocol layer (`nmp_nip18`), NOT in the
/// protocol-agnostic `nmp-feed` engine (D0). One owner, no duplication (D4).
pub use nmp_nip18::PrimaryKindError as FeedParamsError;

/// Validate a [`FeedParams`] declaration's primary kinds and return the compiled
/// acquisition kind set (primary ∪ derived wrappers ∪ kind 5).
///
/// Thin wrapper over the single canonical validator
/// [`nmp_nip18::validate_primary_kinds`] (wrapper/delete/empty rejection + the
/// acquisition derivation). Fail-closed (D6 — typed [`FeedParamsError`], never
/// panic). This is the ONE place the open-feed seam, the FFI/WASM boundary, and
/// the perspective compiler validate primary kinds.
pub fn validate_feed_params(
    params: &FeedParams,
) -> Result<std::collections::BTreeSet<u32>, FeedParamsError> {
    nmp_nip18::validate_primary_kinds(params.primary_kinds.iter().copied())
}

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
    InvalidParams(FeedParamsError),
}

#[cfg(test)]
mod primary_kind_validation_tests {
    use super::*;
    use std::collections::BTreeSet;

    const KIND_DELETE: u32 = 5;

    #[test]
    fn validate_feed_params_accepts_primary_and_derives_wrappers_and_delete() {
        // Delegates to the canonical `nmp_nip18::validate_primary_kinds`: a valid
        // primary declaration compiles to `primary ∪ derived wrappers ∪ kind 5`.
        assert_eq!(
            validate_feed_params(&sample_params(vec![1])),
            Ok(BTreeSet::from([1, 6, KIND_DELETE]))
        );
        assert_eq!(
            validate_feed_params(&sample_params(vec![20])),
            Ok(BTreeSet::from([20, 16, KIND_DELETE]))
        );
    }

    #[test]
    fn validate_feed_params_fails_closed_on_wrapper_delete_empty() {
        assert_eq!(
            validate_feed_params(&sample_params(vec![1, 6])),
            Err(FeedParamsError::RepostWrapper { kind: 6 }),
            "kind 6 is derived acquisition, not primary"
        );
        assert_eq!(
            validate_feed_params(&sample_params(vec![1, KIND_DELETE])),
            Err(FeedParamsError::DeleteKind),
            "kind 5 is derived suppression, not primary"
        );
        assert_eq!(
            validate_feed_params(&sample_params(vec![])),
            Err(FeedParamsError::EmptyPrimaryKinds),
            "an open feed must declare at least one primary kind"
        );
    }

    fn sample_params(primary_kinds: Vec<u32>) -> FeedParams {
        FeedParams {
            primary_kinds,
            render: FeedRender::OpCentric,
            acquisition: FeedScope::ActiveUserFollows,
            admission: FeedAdmission::All,
            ranking: FeedRanking::ChronologicalDesc,
            window: FeedWindow::default(),
            projection: ProjectionKey("nmp.feed.home".into()),
        }
    }
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
                FeedParamsError::RepostWrapper { kind: 6 }
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
