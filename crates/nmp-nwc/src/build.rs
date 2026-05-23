//! Build NIP-47 kind:23194 request event content (NIP-44 encrypted).
//!
//! Returns the JSON-serialized content string and the `p` tag value.
//! The caller (actor wallet runtime) assembles the full `UnsignedEvent`,
//! signs it with the client secret key, and routes it to the NWC relay.

use crate::crypto;
use crate::types::{MakeInvoiceParams, NwcMethod, PayInvoiceParams};
use serde_json::{json, Value};

/// Errors surfaced by `nmp-nwc`'s request-build and crypto layer.
///
/// One enum spans `build` and `crypto` because `build` delegates straight
/// through `crypto` for the encrypt step; a separate `BuildError` /
/// `CryptoError` would force every build call site to add a `From` impl for
/// the same payload. Mirrors [`crate::ParseError`] — a domain-shaped enum
/// with a hand-written [`Display`] impl so call sites can keep their
/// existing `format!("…: {e}")` pattern unchanged after migration.
///
/// Note this is distinct from [`crate::NwcError`], which is the
/// *protocol-level* error returned BY a wallet inside a kind:23195 response
/// (NIP-47 §3) — `NwcBuildError` is the *local* failure mode raised when we
/// fail to construct or encrypt a request in the first place.
///
/// D6 — all variants carry a descriptive payload so the caller can render a
/// toast without losing the underlying cause. No variant unwinds across the
/// FFI seam; every public function returns `Result<_, NwcBuildError>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NwcBuildError {
    /// The supplied client secret hex did not parse as a curve-valid
    /// secp256k1 scalar. Carries the underlying parser message.
    InvalidClientSecret(String),
    /// The supplied wallet pubkey hex did not parse as a curve-valid
    /// secp256k1 x-only pubkey. Carries the underlying parser message.
    InvalidWalletPubkey(String),
    /// `nostr`'s NIP-04 encryptor returned `Err`. Carries the underlying
    /// message.
    Nip04Encrypt(String),
    /// `nostr`'s NIP-04 decryptor returned `Err`. Carries the underlying
    /// message.
    Nip04Decrypt(String),
    /// `nostr`'s NIP-44 decryptor returned `Err`. Carries the underlying
    /// message.
    Nip44Decrypt(String),
    /// A NIP-04 payload was rejected by the local shape validator before
    /// reaching `nostr` (the `?iv=` panic guard — see [`crypto::decrypt`]).
    /// Carries a human-readable reason.
    MalformedNip04Payload(String),
    /// `serde_json` failed to (de)serialize a request body. Carries the
    /// underlying message.
    Json(String),
}

impl std::fmt::Display for NwcBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidClientSecret(msg) => write!(f, "invalid client secret: {msg}"),
            Self::InvalidWalletPubkey(msg) => write!(f, "invalid wallet pubkey: {msg}"),
            Self::Nip04Encrypt(msg) => write!(f, "nip04 encrypt: {msg}"),
            Self::Nip04Decrypt(msg) => write!(f, "nip04 decrypt: {msg}"),
            Self::Nip44Decrypt(msg) => write!(f, "nip44 decrypt: {msg}"),
            Self::MalformedNip04Payload(msg) => write!(f, "malformed nip04 payload: {msg}"),
            Self::Json(msg) => write!(f, "json: {msg}"),
        }
    }
}

impl std::error::Error for NwcBuildError {}

/// Build the NIP-44 encrypted content for a kind:23194 request.
///
/// `client_secret_hex`: client secret from the NWC URI.
/// `wallet_pubkey_hex`: wallet pubkey from the NWC URI.
/// `method`: the NWC method to call.
/// `params`: JSON-serializable params (use `serde_json::Value::Object` or typed structs).
///
/// # Errors
///
/// Returns `NwcBuildError` if the secret or pubkey are not valid secp256k1 keys
/// or if NIP-44 encryption fails.
pub fn request_content(
    client_secret_hex: &str,
    wallet_pubkey_hex: &str,
    method: &NwcMethod,
    params: &Value,
) -> Result<String, NwcBuildError> {
    let json = json!({
        "method": method.as_str(),
        "params": params,
    });
    let plaintext =
        serde_json::to_string(&json).map_err(|e| NwcBuildError::Json(e.to_string()))?;
    crypto::encrypt(client_secret_hex, wallet_pubkey_hex, &plaintext)
}

/// Build encrypted content for a `get_balance` request.
///
/// # Errors
///
/// Returns `NwcBuildError` if key parsing or encryption fails.
pub fn get_balance_content(
    client_secret_hex: &str,
    wallet_pubkey_hex: &str,
) -> Result<String, NwcBuildError> {
    request_content(
        client_secret_hex,
        wallet_pubkey_hex,
        &NwcMethod::GetBalance,
        &json!({}),
    )
}

/// Build encrypted content for a `get_info` request.
///
/// # Errors
///
/// Returns `NwcBuildError` if key parsing or encryption fails.
pub fn get_info_content(
    client_secret_hex: &str,
    wallet_pubkey_hex: &str,
) -> Result<String, NwcBuildError> {
    request_content(
        client_secret_hex,
        wallet_pubkey_hex,
        &NwcMethod::GetInfo,
        &json!({}),
    )
}

/// Build encrypted content for a `pay_invoice` request.
///
/// # Errors
///
/// Returns `NwcBuildError` if key parsing, params serialization, or encryption fails.
pub fn pay_invoice_content(
    client_secret_hex: &str,
    wallet_pubkey_hex: &str,
    params: &PayInvoiceParams,
) -> Result<String, NwcBuildError> {
    let params_value = serde_json::to_value(params)
        .map_err(|e| NwcBuildError::Json(format!("serialize params: {e}")))?;
    request_content(
        client_secret_hex,
        wallet_pubkey_hex,
        &NwcMethod::PayInvoice,
        &params_value,
    )
}

/// Build encrypted content for a `make_invoice` request.
///
/// # Errors
///
/// Returns `NwcBuildError` if key parsing, params serialization, or encryption fails.
pub fn make_invoice_content(
    client_secret_hex: &str,
    wallet_pubkey_hex: &str,
    params: &MakeInvoiceParams,
) -> Result<String, NwcBuildError> {
    let params_value = serde_json::to_value(params)
        .map_err(|e| NwcBuildError::Json(format!("serialize params: {e}")))?;
    request_content(
        client_secret_hex,
        wallet_pubkey_hex,
        &NwcMethod::MakeInvoice,
        &params_value,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto;

    const CLIENT_SECRET: &str =
        "0101010101010101010101010101010101010101010101010101010101010101";
    const WALLET_SECRET: &str =
        "0202020202020202020202020202020202020202020202020202020202020202";

    /// Build a request, then decrypt it back to the inner JSON. The wallet
    /// would decrypt with its own secret; for assertion we round-trip via the
    /// client secret (NIP-04 ECDH is symmetric).
    fn decrypt_built(content: &str, wallet_pk: &str) -> Value {
        let plaintext = crypto::decrypt(CLIENT_SECRET, wallet_pk, content).unwrap();
        serde_json::from_str(&plaintext).unwrap()
    }

    fn wallet_pk() -> String {
        crypto::client_pubkey_hex(WALLET_SECRET).unwrap()
    }

    #[test]
    fn get_balance_request_shape() {
        let wallet_pk = wallet_pk();
        let content = get_balance_content(CLIENT_SECRET, &wallet_pk).unwrap();
        let json = decrypt_built(&content, &wallet_pk);
        assert_eq!(json["method"], "get_balance");
        assert_eq!(json["params"], json!({}));
    }

    #[test]
    fn get_info_request_shape() {
        let wallet_pk = wallet_pk();
        let content = get_info_content(CLIENT_SECRET, &wallet_pk).unwrap();
        let json = decrypt_built(&content, &wallet_pk);
        assert_eq!(json["method"], "get_info");
        assert_eq!(json["params"], json!({}));
    }

    /// `pay_invoice` with an explicit amount override must carry the `amount`
    /// field — a missing amount here would silently change what the user pays.
    #[test]
    fn pay_invoice_request_with_amount() {
        let wallet_pk = wallet_pk();
        let params = PayInvoiceParams {
            invoice: "lnbc1exampleinvoice".to_string(),
            amount: Some(21_000),
        };
        let content = pay_invoice_content(CLIENT_SECRET, &wallet_pk, &params).unwrap();
        let json = decrypt_built(&content, &wallet_pk);
        assert_eq!(json["method"], "pay_invoice");
        assert_eq!(json["params"]["invoice"], "lnbc1exampleinvoice");
        assert_eq!(json["params"]["amount"], 21_000);
    }

    /// With `amount: None`, `skip_serializing_if` must omit the key entirely —
    /// sending `amount: null` could be rejected or misinterpreted by a wallet.
    #[test]
    fn pay_invoice_request_omits_absent_amount() {
        let wallet_pk = wallet_pk();
        let params = PayInvoiceParams {
            invoice: "lnbc1noamount".to_string(),
            amount: None,
        };
        let content = pay_invoice_content(CLIENT_SECRET, &wallet_pk, &params).unwrap();
        let json = decrypt_built(&content, &wallet_pk);
        assert_eq!(json["params"]["invoice"], "lnbc1noamount");
        assert!(
            json["params"].get("amount").is_none(),
            "absent amount must be omitted, not serialized as null"
        );
    }

    #[test]
    fn make_invoice_request_shape() {
        let wallet_pk = wallet_pk();
        let params = MakeInvoiceParams {
            amount: 5_000,
            description: Some("coffee".to_string()),
            expiry: None,
        };
        let content = make_invoice_content(CLIENT_SECRET, &wallet_pk, &params).unwrap();
        let json = decrypt_built(&content, &wallet_pk);
        assert_eq!(json["method"], "make_invoice");
        assert_eq!(json["params"]["amount"], 5_000);
        assert_eq!(json["params"]["description"], "coffee");
        assert!(
            json["params"].get("expiry").is_none(),
            "absent expiry must be omitted"
        );
    }

    /// An invalid wallet pubkey must propagate as Err from the build layer,
    /// never panic — D6.
    #[test]
    fn build_with_invalid_pubkey_errs() {
        assert!(get_balance_content(CLIENT_SECRET, "not-a-pubkey").is_err());
    }

    /// The typed enum's `Display` impl must produce the same wire-string
    /// shape the previous `String`-typed surface used, so a caller's
    /// `format!("...: {e}")` toast text is byte-identical post-migration.
    #[test]
    fn display_matches_legacy_string_format() {
        // Invalid pubkey → "invalid wallet pubkey: <underlying>"
        let err = get_balance_content(CLIENT_SECRET, "not-a-pubkey").unwrap_err();
        let rendered = format!("{err}");
        assert!(
            rendered.starts_with("invalid wallet pubkey: "),
            "Display prefix must match legacy String, got: {rendered}"
        );
        // The Debug impl is also derived so test fixtures can use it.
        let _dbg = format!("{err:?}");
    }
}
