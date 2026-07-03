//! Pure Cashu mint HTTP request construction and response validation
//! (NUT-03/04/05/07/12).
//!
//! # Why this module exists (the "capability lane")
//!
//! `nmp-core` never learns Cashu/mint vocabulary (D0) and `nmp-nip60` never
//! learns how a request actually reaches the wire (that differs by
//! runtime — native `ureq` on a worker thread vs. a browser `fetch()`
//! executed through `nmp-core`'s generic
//! `nmp_core::substrate::OutboundHttpCapability`). This module is the seam
//! between those two facts: everything here is **pure** — no I/O, no
//! `ureq`, always compiled (including `wasm32`) — and speaks only
//! [`MintHttpRequest`] / [`MintRawResponse`], a transport-neutral pair any
//! runtime can produce and consume:
//!
//! - `build_*` / `prepare_*` — construct the wire request (path + JSON body)
//!   from typed Cashu inputs. `prepare_*` additionally returns the
//!   client-side secret state (blinding factors, generated proof secrets)
//!   the matching `finalize_*` needs to unblind the mint's response — this
//!   is money-adjacent state, so it travels in one typed value instead of
//!   being threaded through mutable fields.
//! - `parse_*` / `finalize_*` — validate + decode a [`MintRawResponse`] into
//!   a typed Cashu result. This is where a hostile or buggy mint response
//!   (wrong signature count, wrong amount, wrong keyset id, tampered DLEQ
//!   proof, an HTML error page instead of JSON) is rejected. These functions
//!   run identically regardless of whether the raw bytes arrived via a
//!   native blocking HTTP call or a browser capability round-trip — the
//!   transport cannot smuggle a differently-validated code path.
//!
//! The native, blocking transport (`ureq`, `native` feature only) lives in
//! [`super::client`], which calls into this module for request/response
//! shape and keeps none of the validation logic itself. A future
//! `nmp-wallet` browser adapter maps [`MintHttpRequest`] to
//! `nmp_core::substrate::OutboundHttpRequest`, executes it through the
//! capability socket, maps the returned `OutboundHttpResult` back to a
//! [`MintRawResponse`], and calls the same `finalize_*`/`parse_*` functions.
//!
//! # No secret material in `Debug`/logs
//!
//! [`MintHttpRequest`]/[`MintRawResponse`] bodies routinely carry proof
//! secrets, blinding factors, and quote ids. Both types implement a redacted
//! `Debug` (path/body hidden, only operation/method/status/length shown) —
//! see `sensitive_debug_redacts_*` tests.
//!
//! # Module layout (file-size discipline)
//!
//! This file owns only the transport-neutral request/response shell and the
//! shared JSON/error-envelope helpers every operation uses. Each Cashu
//! operation family lives in its own submodule:
//!
//! - [`keyset`] — `/v1/keys` + `/v1/keysets` (NUT-01/NUT-02).
//! - [`quote`] — mint-quote request/status (NUT-04/NUT-23).
//! - [`blinded`] — the shared blind/unblind + DLEQ-verify engine minting and
//!   swapping both build on.
//! - [`mint`] — `/v1/mint/bolt11` (NUT-04).
//! - [`swap`] — `/v1/swap` (NUT-03).
//! - [`checkstate`] — `/v1/checkstate` (NUT-07).
//!
//! `#[cfg(test)] mint_http_support` holds test-only fixtures shared across
//! those submodules' own test files.

mod blinded;
mod checkstate;
mod keyset;
mod mint;
mod quote;
mod swap;

#[cfg(test)]
mod mint_http_support;

pub use blinded::split_amount;
pub use checkstate::{build_check_state_request, parse_check_state_response};
pub use keyset::{build_get_keys_request, build_get_keysets_request, parse_keys_response};
// Re-exported for `nutzap::verify_nutzap_dleq` (native-only); unused (and
// hence warning-worthy) in a `--no-default-features` build.
#[cfg_attr(not(feature = "native"), allow(unused_imports))]
pub(crate) use keyset::build_pubkey_map;
pub use mint::{
    finalize_mint_bolt11_response, prepare_mint_bolt11_request, PreparedMintBolt11Request,
};
pub use quote::{
    build_get_mint_quote_bolt11_request, build_mint_quote_bolt11_request,
    parse_mint_quote_bolt11_response, MintQuoteExpectation,
};
pub use swap::{finalize_swap_response, prepare_swap_request, PreparedSwapRequest};

use std::fmt;

/// Placeholder printed in place of a redacted `Debug` field. Shared by every
/// submodule's hand-written `Debug` impl.
pub(super) const REDACTED: &str = "<redacted>";

// ─── Transport-neutral request/response ────────────────────────────────────

/// HTTP method for a [`MintHttpRequest`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MintHttpMethod {
    Get,
    Post,
}

/// Which Cashu mint HTTP operation a [`MintHttpRequest`] carries. Threaded
/// through so a worker can log/trace *which* operation is in flight without
/// ever needing the URL or body (see [`MintHttpRequest`]'s `Debug` impl).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MintHttpOperation {
    GetKeys,
    GetKeysets,
    CreateMintQuoteBolt11,
    GetMintQuoteBolt11,
    MintBolt11,
    Swap,
    CheckState,
    CreateMeltQuoteBolt11,
    GetMeltQuoteBolt11,
    MeltBolt11,
}

/// A fully-constructed Cashu mint HTTP request, ready for either a native
/// blocking `ureq` call or a browser capability round-trip. `path` is
/// relative (e.g. `/v1/mint/quote/bolt11`); the transport prepends the mint
/// base URL.
pub struct MintHttpRequest {
    pub operation: MintHttpOperation,
    pub method: MintHttpMethod,
    pub path: String,
    pub body: Vec<u8>,
}

impl fmt::Debug for MintHttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `path` can carry a quote id (e.g. `/v1/mint/quote/bolt11/<id>`);
        // `body` carries proof secrets / blinding factors. Neither is safe
        // to print unconditionally.
        f.debug_struct("MintHttpRequest")
            .field("operation", &self.operation)
            .field("method", &self.method)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// The raw HTTP response for a [`MintHttpRequest`], before any Cashu-level
/// validation. `status_code` may be any HTTP status — `parse_*`/`finalize_*`
/// decide what a given status means for a given operation.
pub struct MintRawResponse {
    pub status_code: u16,
    pub body: Vec<u8>,
}

impl fmt::Debug for MintRawResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MintRawResponse")
            .field("status_code", &self.status_code)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// Whether a mint's response is required to carry a NUT-12 DLEQ proof.
/// `VerifyIfPresent` matches this crate's historical, mint-compatible
/// behaviour (verify when offered, accept its absence); a caller that has
/// out-of-band confirmed a mint advertises NUT-12 support may pass `Require`
/// to fail closed on a mint that silently stops sending proofs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DleqPolicy {
    VerifyIfPresent,
    Require,
}

/// Bound on how much of an error response body we'll fold into a
/// [`crate::error::Nip60Error`] message — a misbehaving mint should not be
/// able to make our error strings (and downstream logs) unbounded.
const MAX_ERROR_BODY_ECHO: usize = 512;

/// Turn a non-2xx status + raw body into a [`crate::error::Nip60Error`].
/// Tries to parse the body as the Cashu `{"code":N,"detail":"..."}` error
/// shape (NUT-00 "Error Handling"); anything else (an HTML error page, empty
/// body, plain text) is treated as an opaque transport-level failure and
/// never echoed verbatim (only its length), since it did not come from the
/// Cashu protocol's own error vocabulary.
pub(super) fn mint_error_from_body(status_code: u16, body: &[u8]) -> crate::error::Nip60Error {
    #[derive(serde::Deserialize)]
    struct CashuError {
        #[serde(default)]
        detail: Option<String>,
        #[serde(default)]
        error: Option<String>,
    }
    if let Ok(err) = serde_json::from_slice::<CashuError>(body) {
        if let Some(detail) = err.detail.or(err.error) {
            let truncated: String = detail.chars().take(MAX_ERROR_BODY_ECHO).collect();
            return crate::error::Nip60Error::MintProtocol(format!(
                "mint error (status {status_code}): {truncated}"
            ));
        }
    }
    crate::error::Nip60Error::MintHttp(format!(
        "mint returned status {status_code} with a non-Cashu-error body ({} bytes)",
        body.len()
    ))
}

pub(super) fn parse_json_response<T: serde::de::DeserializeOwned>(
    raw: &MintRawResponse,
    what: &str,
) -> Result<T, crate::error::Nip60Error> {
    if !(200..300).contains(&raw.status_code) {
        return Err(mint_error_from_body(raw.status_code, &raw.body));
    }
    serde_json::from_slice(&raw.body)
        .map_err(|e| crate::error::Nip60Error::MintProtocol(format!("{what} decode: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D6/security — a stray `{:?}` on a mint HTTP request/response must
    /// never leak the path (which can carry a quote id) or the body (which
    /// can carry a proof secret or blinding factor).
    #[test]
    fn sensitive_debug_redacts_body() {
        let req = MintHttpRequest {
            operation: MintHttpOperation::MintBolt11,
            method: MintHttpMethod::Post,
            path: "/v1/mint/bolt11/super-secret-quote".to_string(),
            body: b"{\"secret\":\"leak-me-not\"}".to_vec(),
        };
        let debug = format!("{req:?}");
        assert!(!debug.contains("super-secret-quote"));
        assert!(!debug.contains("leak-me-not"));

        let raw = MintRawResponse {
            status_code: 200,
            body: b"{\"secret\":\"leak-me-not\"}".to_vec(),
        };
        let debug = format!("{raw:?}");
        assert!(!debug.contains("leak-me-not"));
    }
}
