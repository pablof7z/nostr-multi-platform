//! Build NIP-47 kind:23194 request event content (NIP-44 encrypted).
//!
//! Returns the JSON-serialized content string and the `p` tag value.
//! The caller (actor wallet runtime) assembles the full `UnsignedEvent`,
//! signs it with the client secret key, and routes it to the NWC relay.

use crate::crypto;
use crate::types::{MakeInvoiceParams, NwcMethod, PayInvoiceParams};
use serde_json::{json, Value};

/// Build the NIP-44 encrypted content for a kind:23194 request.
///
/// `client_secret_hex`: client secret from the NWC URI.
/// `wallet_pubkey_hex`: wallet pubkey from the NWC URI.
/// `method`: the NWC method to call.
/// `params`: JSON-serializable params (use `serde_json::Value::Object` or typed structs).
pub fn request_content(
    client_secret_hex: &str,
    wallet_pubkey_hex: &str,
    method: &NwcMethod,
    params: Value,
) -> Result<String, String> {
    let json = json!({
        "method": method.as_str(),
        "params": params,
    });
    let plaintext = serde_json::to_string(&json).map_err(|e| format!("json: {e}"))?;
    crypto::encrypt(client_secret_hex, wallet_pubkey_hex, &plaintext)
}

/// Build encrypted content for a `get_balance` request.
pub fn get_balance_content(
    client_secret_hex: &str,
    wallet_pubkey_hex: &str,
) -> Result<String, String> {
    request_content(
        client_secret_hex,
        wallet_pubkey_hex,
        &NwcMethod::GetBalance,
        json!({}),
    )
}

/// Build encrypted content for a `get_info` request.
pub fn get_info_content(
    client_secret_hex: &str,
    wallet_pubkey_hex: &str,
) -> Result<String, String> {
    request_content(
        client_secret_hex,
        wallet_pubkey_hex,
        &NwcMethod::GetInfo,
        json!({}),
    )
}

/// Build encrypted content for a `pay_invoice` request.
pub fn pay_invoice_content(
    client_secret_hex: &str,
    wallet_pubkey_hex: &str,
    params: PayInvoiceParams,
) -> Result<String, String> {
    let params_value =
        serde_json::to_value(&params).map_err(|e| format!("serialize params: {e}"))?;
    request_content(
        client_secret_hex,
        wallet_pubkey_hex,
        &NwcMethod::PayInvoice,
        params_value,
    )
}

/// Build encrypted content for a `make_invoice` request.
pub fn make_invoice_content(
    client_secret_hex: &str,
    wallet_pubkey_hex: &str,
    params: MakeInvoiceParams,
) -> Result<String, String> {
    let params_value =
        serde_json::to_value(&params).map_err(|e| format!("serialize params: {e}"))?;
    request_content(
        client_secret_hex,
        wallet_pubkey_hex,
        &NwcMethod::MakeInvoice,
        params_value,
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
        let content = pay_invoice_content(CLIENT_SECRET, &wallet_pk, params).unwrap();
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
        let content = pay_invoice_content(CLIENT_SECRET, &wallet_pk, params).unwrap();
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
        let content = make_invoice_content(CLIENT_SECRET, &wallet_pk, params).unwrap();
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
}
