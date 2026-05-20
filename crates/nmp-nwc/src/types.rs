//! NIP-47 NWC request/response types.

use serde::{Deserialize, Serialize};

// ── Request ───────────────────────────────────────────────────────────────────

/// Supported NWC request methods.
#[derive(Debug, Clone, PartialEq)]
pub enum NwcMethod {
    GetInfo,
    GetBalance,
    PayInvoice,
    MakeInvoice,
}

impl NwcMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::GetInfo => "get_info",
            Self::GetBalance => "get_balance",
            Self::PayInvoice => "pay_invoice",
            Self::MakeInvoice => "make_invoice",
        }
    }
}

/// Parameters for `pay_invoice`.
#[derive(Debug, Clone, Serialize)]
pub struct PayInvoiceParams {
    pub invoice: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<u64>,
}

/// Parameters for `make_invoice`.
#[derive(Debug, Clone, Serialize)]
pub struct MakeInvoiceParams {
    pub amount: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiry: Option<u64>,
}

// ── Response ──────────────────────────────────────────────────────────────────

/// Envelope returned by the wallet service (decrypted from kind:23195 content).
#[derive(Debug, Clone, Deserialize)]
pub struct NwcResponse {
    pub result_type: String,
    pub error: Option<NwcError>,
    pub result: Option<serde_json::Value>,
}

/// NWC error object from the wallet service.
#[derive(Debug, Clone, Deserialize)]
pub struct NwcError {
    pub code: String,
    pub message: String,
}

/// Decoded `get_balance` result.
#[derive(Debug, Clone, Deserialize)]
pub struct GetBalanceResult {
    pub balance: u64,
}

/// Decoded `get_info` result.
#[derive(Debug, Clone, Deserialize)]
pub struct GetInfoResult {
    pub alias: Option<String>,
    pub color: Option<String>,
    pub pubkey: Option<String>,
    pub network: Option<String>,
    pub methods: Vec<String>,
}

/// Decoded `pay_invoice` result.
#[derive(Debug, Clone, Deserialize)]
pub struct PayInvoiceResult {
    pub preimage: String,
}

/// Decoded `make_invoice` result.
#[derive(Debug, Clone, Deserialize)]
pub struct MakeInvoiceResult {
    pub invoice: String,
    pub payment_hash: Option<String>,
}

impl NwcResponse {
    /// Extract balance in msats from a `get_balance` response.
    pub fn balance_msats(&self) -> Option<u64> {
        if self.result_type != "get_balance" || self.error.is_some() {
            return None;
        }
        self.result
            .as_ref()
            .and_then(|v| serde_json::from_value::<GetBalanceResult>(v.clone()).ok())
            .map(|r| r.balance)
    }

    /// Extract the payment preimage from a `pay_invoice` response.
    pub fn pay_preimage(&self) -> Option<String> {
        if self.result_type != "pay_invoice" || self.error.is_some() {
            return None;
        }
        self.result
            .as_ref()
            .and_then(|v| serde_json::from_value::<PayInvoiceResult>(v.clone()).ok())
            .map(|r| r.preimage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn nwc_method_strings_match_nip47() {
        assert_eq!(NwcMethod::GetInfo.as_str(), "get_info");
        assert_eq!(NwcMethod::GetBalance.as_str(), "get_balance");
        assert_eq!(NwcMethod::PayInvoice.as_str(), "pay_invoice");
        assert_eq!(NwcMethod::MakeInvoice.as_str(), "make_invoice");
    }

    fn response(result_type: &str, error: Option<NwcError>, result: serde_json::Value)
        -> NwcResponse
    {
        NwcResponse {
            result_type: result_type.to_string(),
            error,
            result: Some(result),
        }
    }

    #[test]
    fn balance_msats_reads_get_balance_result() {
        let r = response("get_balance", None, json!({ "balance": 777_u64 }));
        assert_eq!(r.balance_msats(), Some(777));
    }

    /// A `get_balance` accessor on a `pay_invoice` response must return None —
    /// guards against reading a balance off the wrong result shape.
    #[test]
    fn balance_msats_wrong_result_type_is_none() {
        let r = response("pay_invoice", None, json!({ "balance": 777_u64 }));
        assert_eq!(r.balance_msats(), None);
    }

    /// Even with a populated `result`, an error response must yield None — the
    /// wallet did not actually return a usable balance.
    #[test]
    fn balance_msats_with_error_is_none() {
        let err = NwcError { code: "INTERNAL".into(), message: "boom".into() };
        let r = response("get_balance", Some(err), json!({ "balance": 777_u64 }));
        assert_eq!(r.balance_msats(), None);
    }

    #[test]
    fn pay_preimage_reads_pay_invoice_result() {
        let r = response("pay_invoice", None, json!({ "preimage": "deadbeef" }));
        assert_eq!(r.pay_preimage(), Some("deadbeef".to_string()));
    }

    #[test]
    fn pay_preimage_wrong_result_type_is_none() {
        let r = response("get_balance", None, json!({ "preimage": "deadbeef" }));
        assert_eq!(r.pay_preimage(), None);
    }

    /// A failed payment must never surface a preimage — that would falsely
    /// signal the payment settled.
    #[test]
    fn pay_preimage_with_error_is_none() {
        let err = NwcError {
            code: "PAYMENT_FAILED".into(),
            message: "no route".into(),
        };
        let r = response("pay_invoice", Some(err), json!({ "preimage": "deadbeef" }));
        assert_eq!(r.pay_preimage(), None);
    }

    /// `result_type` matches but `result` is absent / malformed → None, no panic.
    #[test]
    fn accessors_handle_missing_or_malformed_result() {
        let no_result = NwcResponse {
            result_type: "get_balance".into(),
            error: None,
            result: None,
        };
        assert_eq!(no_result.balance_msats(), None);

        let bad_shape = response("get_balance", None, json!({ "wrong_field": 1 }));
        assert_eq!(bad_shape.balance_msats(), None);
    }
}
