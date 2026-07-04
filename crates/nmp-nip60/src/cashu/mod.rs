//! Cashu mint HTTP client and cryptographic primitives.
//!
//! # Modules
//!
//! - [`crypto`] — DHKE blind signatures and DLEQ proof verification (NUT-00, NUT-12).
//! - [`types`] — HTTP API request/response types (NUT-01 through NUT-12).
//! - [`http`] — pure mint HTTP request construction + response validation
//!   (the "capability lane" — always compiled, no `ureq`; see its module
//!   docs for why native and browser transports both funnel through it).
//! - `client` — Synchronous HTTP client wrapping [`http`] with `ureq`.
//!   Requires the `native` feature; [`http`]'s pure builder/validator
//!   surface stays HTTP-free and always-compiled so a browser transport can
//!   reuse the exact same validation.

#[cfg(feature = "native")]
pub mod client;
pub mod crypto;
pub mod http;
pub mod types;

#[cfg(feature = "native")]
pub use client::MintClient;
pub use crypto::{
    blind_message, hash_to_curve, random_secret, random_secret_hex, unblind_signature, verify_dleq,
    DleqProof,
};
pub use http::{
    build_check_state_request, build_get_keys_request, build_get_keysets_request,
    build_get_mint_quote_bolt11_request, build_mint_quote_bolt11_request,
    finalize_mint_bolt11_response, finalize_swap_response, parse_check_state_response,
    parse_keys_response, parse_mint_quote_bolt11_response, prepare_mint_bolt11_request,
    prepare_swap_request, split_amount, DleqPolicy, MintHttpMethod, MintHttpOperation,
    MintHttpRequest, MintQuoteExpectation, MintRawResponse, PreparedMintBolt11Request,
    PreparedSwapRequest,
};
pub use types::Proof;

/// Normalize a Cashu mint HTTP URL for equality comparisons (#2975, mirroring
/// #2972's `nmp-wallet` fix) — two strings that name the same real mint (a
/// trailing slash, a differently-cased scheme/host) must compare equal
/// wherever this crate matches a caller-resolved mint URL against a stored
/// token record's mint.
///
/// This is a deliberate duplicate of
/// `nmp_wallet::backend::cashu::state::canonicalize_mint_url`, not a shared
/// import: `nmp-nip60` is a dependency OF `nmp-wallet` (never the reverse),
/// so `nmp-wallet` cannot be a new dependency here, and standing up a shared
/// crate for one ~15-line pure string helper is more architecture than this
/// fix warrants. Keep both copies in sync if the normalization rule ever
/// changes.
///
/// Deliberately narrower than a general URL-canonicalization routine: a Cashu
/// mint URL's PATH is semantically load-bearing (e.g. minibits serves a
/// distinct endpoint per unit at `/Bitcoin`), so only the scheme and host are
/// lowercased and only a single trailing `/` is stripped from the path — the
/// path's case and interior segments (and any additional trailing slashes)
/// are preserved untouched. Falls back to the trimmed, unmodified input when
/// the string has no `scheme://` separator — never panics, never invents a
/// canonical form for a non-URL.
///
/// Only called from `nip60_wallet::nutzap_send` (`native`-only, since every
/// caller round-trips to a mint over HTTP); the `allow` keeps a
/// `--no-default-features` build warning-free.
#[cfg_attr(not(feature = "native"), allow(dead_code))]
pub(crate) fn canonicalize_mint_url(mint: &str) -> String {
    let trimmed = mint.trim();
    let Some((scheme, rest)) = trimmed.split_once("://") else {
        return trimmed.to_string();
    };
    let scheme = scheme.to_ascii_lowercase();
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = rest[..authority_end].to_ascii_lowercase();
    let mut remainder = rest[authority_end..].to_string();
    let path_end = remainder.find(['?', '#']).unwrap_or(remainder.len());
    if path_end > 0 && remainder.as_bytes()[path_end - 1] == b'/' {
        remainder.remove(path_end - 1);
    }
    format!("{scheme}://{authority}{remainder}")
}

#[cfg(test)]
mod canonicalize_mint_url_tests {
    use super::canonicalize_mint_url;

    #[test]
    fn strips_a_single_trailing_slash() {
        assert_eq!(
            canonicalize_mint_url("https://mint.example/Bitcoin/"),
            "https://mint.example/Bitcoin"
        );
    }

    #[test]
    fn lowercases_scheme_and_host_only() {
        assert_eq!(
            canonicalize_mint_url("HTTPS://Mint.Example/Bitcoin"),
            "https://mint.example/Bitcoin"
        );
    }

    #[test]
    fn preserves_a_double_trailing_slash_as_distinct() {
        assert_eq!(
            canonicalize_mint_url("https://mint.example/Bitcoin//"),
            "https://mint.example/Bitcoin/"
        );
    }

    #[test]
    fn falls_back_to_trimmed_input_without_a_scheme_separator() {
        assert_eq!(canonicalize_mint_url("  not-a-url  "), "not-a-url");
    }
}
