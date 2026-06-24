//! App-facing feed declaration helpers for [`super::WasmRuntime`].
//!
//! The public surface accepts primary content kinds. Protocol wrapper
//! acquisition is derived here, below app composition and above the pure
//! reducer, so apps do not compile repost shapes themselves.
//!
//! #1740 step 1 adds the typed feed-session param model from `nmp-feed`
//! (single source, D4) plus a wasm-boundary decode/validation entry.
//! Step 1 is types + decode + validation only; the `open_feed` dispatch
//! and the public re-export surface land in step 2.

use super::WasmRuntime;

// Import the protocol-agnostic typed feed-session model. The full public
// re-export surface (CustomPerspectiveId, FeedAdmission, FeedHandle, etc.) will
// be hoisted through the wasm crate facade when the `open_feed` wasm dispatch
// lands in step 2. Only what this file actually references is imported here.
//
// Primary-kind validation (which kinds are derived acquisition vs. primary
// input) is protocol knowledge and is NOT in `nmp-feed`; it rides on the single
// `nmp_nip18` transform, composed at this boundary (D0).
use nmp_feed::FeedParams;

/// Typed error for the wasm-boundary `FeedParams` decode + validation
/// (D6 — no panic; the wasm worker reports the variant, never throws).
///
/// This is the single canonical primary-kind validation error
/// [`nmp_nip18::PrimaryKindError`] — the SAME owner the native FFI boundary and
/// the perspective compiler use (D4 — no duplicated validator/error).
pub use nmp_nip18::PrimaryKindError as FeedParamsError;

/// Typed error for the wasm-boundary `FeedParams` decode + validation
/// (D6 — no panic; the wasm worker reports the variant, never throws).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeedParamsDecodeError {
    /// The JSON payload did not parse into a `FeedParams`.
    MalformedJson,
    /// The decoded params failed validation (e.g. wrapper/delete primary kind).
    InvalidParams(FeedParamsError),
}

/// Validate a [`FeedParams`] declaration's primary kinds and return the compiled
/// acquisition kind set (primary ∪ derived wrappers ∪ kind 5), or a typed error.
///
/// Thin wrapper over the single canonical validator
/// [`nmp_nip18::validate_primary_kinds`] (wrapper/delete/empty rejection + the
/// acquisition derivation) — the SAME owner native FFI uses.
fn validate_feed_params(
    params: &FeedParams,
) -> Result<std::collections::BTreeSet<u32>, FeedParamsError> {
    nmp_nip18::validate_primary_kinds(params.primary_kinds.iter().copied())
}

/// Decode a `FeedParams` JSON payload from the browser worker and validate its
/// primary kinds.
///
/// The wasm twin of the FFI boundary entry: parses untrusted JSON into the
/// typed [`FeedParams`] and runs fail-closed primary-kind validation (rejecting
/// wrapper kinds 6/16 and delete kind 5). It performs **no** dispatch — step 2
/// wires `open_feed` on top of this.
pub fn decode_and_validate_feed_params(
    json: &str,
) -> Result<(FeedParams, std::collections::BTreeSet<u32>), FeedParamsDecodeError> {
    let params: FeedParams =
        serde_json::from_str(json).map_err(|_| FeedParamsDecodeError::MalformedJson)?;
    let acquisition_kinds =
        validate_feed_params(&params).map_err(FeedParamsDecodeError::InvalidParams)?;
    Ok((params, acquisition_kinds))
}

impl WasmRuntime {
    /// Declare an active-follows feed from app-owned primary content kinds.
    ///
    /// This is the wasm twin of `NmpApp::declare_active_follows_feed`.
    /// Callers name only the primary kinds they intend to render. NIP-18
    /// repost wrappers are derived here before the pure reducer receives the
    /// compiled acquisition set, so app composition never has to say
    /// "kind 1 plus kind 6" or "kind 20 plus kind 16".
    ///
    /// Returns `false` when a caller supplies a wrapper kind as primary input
    /// or otherwise fails NIP-18 primary-kind validation. The reducer is left
    /// unchanged on failure.
    pub fn declare_active_follows_feed<I>(&self, primary_kinds: I) -> bool
    where
        I: IntoIterator<Item = u32>,
    {
        let Ok(acquisition_kinds) = nmp_nip18::try_acquisition_kinds_for_primary(primary_kinds)
        else {
            return false;
        };
        let outbound = self
            .reducer
            .borrow_mut()
            .declare_active_follows_feed(acquisition_kinds);
        self.fan_outbound(outbound);
        self.request_event_drain();
        true
    }

    /// Clear the active-follows feed declaration.
    pub fn clear_active_follows_feed(&self) {
        let outbound = self.reducer.borrow_mut().clear_active_follows_feed();
        self.fan_outbound(outbound);
        self.request_event_drain();
    }
}

#[cfg(test)]
mod feed_params_decode_tests {
    use super::*;
    use nmp_feed::PubkeySetExpr;

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
            decode_and_validate_feed_params(&params_json("[20]")).expect("[20] is valid");
        assert_eq!(params.primary_kinds, vec![20]);
        assert!(kinds.contains(&20) && kinds.contains(&16));
        assert_eq!(params.acquisition, PubkeySetExpr::ActiveUserFollows);
    }

    #[test]
    fn wrapper_and_delete_primary_kinds_are_rejected_at_the_boundary() {
        assert_eq!(
            decode_and_validate_feed_params(&params_json("[20, 16]")),
            Err(FeedParamsDecodeError::InvalidParams(
                FeedParamsError::RepostWrapper { kind: 16 }
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
