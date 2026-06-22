//! NIP-47 (NWC) user-facing error codes (issue #1682).
//!
//! Stable machine keys carried by [`nmp_core::ui_token::UiToken::code`] so the
//! shells render localized prose instead of the English fallback. The set is
//! CLOSED — adding a key is a deliberate FFI-contract change. The shells map
//! every key here to localized copy; an unknown key falls back to the token's
//! English `fallback_prose`.

/// The supplied NWC connection URI was structurally invalid.
pub const INVALID_URI: &str = "nip47_invalid_uri";

/// The NWC client secret in the URI was invalid (bad hex / key).
pub const INVALID_CLIENT_SECRET: &str = "nip47_invalid_client_secret";

/// Encoding the kind:23195 REQ subscription frame failed.
pub const REQ_ENCODE_FAILED: &str = "nip47_req_encode_failed";

/// Encrypting an NWC request payload failed.
pub const ENCRYPT_FAILED: &str = "nip47_encrypt_failed";

/// Signing an NWC request event failed.
pub const SIGN_FAILED: &str = "nip47_sign_failed";

/// Encoding the NWC EVENT frame failed.
pub const EVENT_ENCODE_FAILED: &str = "nip47_event_encode_failed";

/// The wallet service returned an error response. `subject` carries the wallet
/// error code; `raw_detail` the wallet error message.
pub const WALLET_ERROR: &str = "nip47_wallet_error";

/// The wallet service rejected the request as unauthorized/restricted.
/// `subject` carries the wallet error code; `raw_detail` the message.
pub const WALLET_AUTH_ERROR: &str = "nip47_wallet_auth_error";

/// A pay request was attempted while the wallet is still connecting.
pub const WALLET_NOT_READY: &str = "nip47_wallet_not_ready";

/// A pay request was attempted with no wallet connected.
pub const WALLET_NOT_CONNECTED: &str = "nip47_wallet_not_connected";

/// A payment was aborted because its durable record could not be written
/// (refusing to risk a double-pay on restart). `raw_detail` carries the
/// storage error.
pub const PAYMENT_ABORTED_NO_DURABLE_RECORD: &str = "nip47_payment_aborted_no_durable_record";

// ── connect-action rejection codes (#1734) ───────────────────────────────────
// Stable machine keys for the `nmp.wallet.connect` `start()` rejections.
// These ride the inline action-result JSON (`{"error":"…","code":"…"}`) via
// `ActionRejection::InvalidCoded`, NOT the toast/progress wire.  Shells map
// each key to localized copy; an unknown key falls back to the English
// `message` in the rejection.

/// The NWC URI supplied to `nmp.wallet.connect` was empty.
pub const NWC_URI_EMPTY: &str = "nip47_nwc_uri_empty";

/// The NWC URI supplied to `nmp.wallet.connect` had the wrong scheme (must
/// start with `nostr+walletconnect://`).
pub const NWC_URI_BAD_SCHEME: &str = "nip47_nwc_uri_bad_scheme";
