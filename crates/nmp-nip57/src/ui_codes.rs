//! NIP-57 (Zap) user-facing error codes (issue #2285 / #1682).
//!
//! Stable machine keys carried by [`nmp_core::ui_token::UiToken::code`] so the
//! shells render localized prose instead of the English fallback. The set is
//! CLOSED — adding a key is a deliberate FFI-contract change. The shells map
//! every key here to localized copy; an unknown key falls back to the token's
//! English `fallback_prose`.

/// The zap recipient has no lightning address in their kind:0 profile; the
/// kernel could not resolve an LNURL to invoke.
pub const ZAP_NO_LNURL: &str = "nip57_zap_no_lnurl";

/// The LNURL / lightning-address could not be resolved to a well-known
/// callback URL (bech32 decode, DNS, or HTTP leg 1 failure). `raw_detail`
/// carries the resolver reason.
pub const ZAP_LNURL_RESOLVE_FAILED: &str = "nip57_zap_lnurl_resolve_failed";

/// The kind:9734 zap-request event could not be signed (genuinely absent
/// account, broker rejection, or JSON serialization failure). `raw_detail`
/// carries the sign/serialize reason.
pub const ZAP_SIGN_FAILED: &str = "nip57_zap_sign_failed";

/// The provider's pending-zap registry write failed, OR an unexpected
/// protocol-level failure during the LNURL pay round-trip. `raw_detail`
/// carries the reason.
pub const ZAP_FAILED: &str = "nip57_zap_failed";

/// A zap was attempted but no NWC wallet is wired into this app instance.
pub const ZAP_NO_WALLET: &str = "nip57_zap_no_wallet";

/// The LNURL-pay HTTP round-trip (leg 1 GET or leg 2 callback GET) failed.
/// `raw_detail` carries the HTTP / network reason.
pub const ZAP_FETCH_FAILED: &str = "nip57_zap_fetch_failed";
