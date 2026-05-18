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
