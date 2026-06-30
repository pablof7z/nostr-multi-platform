//! NIP-47 NWC request/response types.
//!
//! ## Why not `nostr::nips::nip47::{Method, Response, ResponseResult}`?
//!
//! The `nostr` crate's `nip47` module models the full NWC surface as a tight
//! `ResponseResult` enum keyed on a known method set. `NwcResponse` here is
//! deliberately *loose*: `result_type: String` + `result: serde_json::Value`.
//! That shape decodes **any** wallet's response — including methods/fields this
//! client does not yet model — without a wire-compat break. Tightening to the
//! upstream enum would risk rejecting responses from deployed wallets (Alby,
//! Mutiny, Zeus) that return extra or differently-shaped fields. The typed
//! accessors (`balance_msats`, `pay_preimage`) provide the narrow, validated
//! reads the actor runtime actually needs. Keep loose; do not enable `nip47`.

use serde::{Deserialize, Serialize};

// ── Request ───────────────────────────────────────────────────────────────────

/// Supported NWC request methods.
#[derive(Debug, Clone, PartialEq)]
pub enum NwcMethod {
    GetInfo,
    GetBalance,
    PayInvoice,
    LookupInvoice,
}

impl NwcMethod {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::GetInfo => "get_info",
            Self::GetBalance => "get_balance",
            Self::PayInvoice => "pay_invoice",
            Self::LookupInvoice => "lookup_invoice",
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

/// Parameters for `lookup_invoice`. NIP-47 accepts payment_hash OR invoice (bolt11).
#[derive(Debug, Clone, Serialize)]
pub struct LookupInvoiceParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invoice: Option<String>,
}

/// Decoded `lookup_invoice` result.
#[derive(Debug, Clone, Deserialize)]
pub struct LookupInvoiceResult {
    pub payment_hash: String,
    /// `"settled"` when payment succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preimage: Option<String>,
}

impl NwcResponse {
    /// Extract balance in msats from a `get_balance` response.
    #[must_use]
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
    #[must_use]
    pub fn pay_preimage(&self) -> Option<String> {
        if self.result_type != "pay_invoice" || self.error.is_some() {
            return None;
        }
        self.result
            .as_ref()
            .and_then(|v| serde_json::from_value::<PayInvoiceResult>(v.clone()).ok())
            .map(|r| r.preimage)
    }

    /// Extract a `lookup_invoice` result. Returns None when result_type mismatches,
    /// error is set, or result is absent/malformed.
    #[must_use]
    pub fn lookup_invoice_result(&self) -> Option<LookupInvoiceResult> {
        if self.result_type != "lookup_invoice" || self.error.is_some() {
            return None;
        }
        self.result
            .as_ref()
            .and_then(|v| serde_json::from_value::<LookupInvoiceResult>(v.clone()).ok())
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
    }

    #[test]
    fn lookup_invoice_method_as_str_is_correct() {
        assert_eq!(NwcMethod::LookupInvoice.as_str(), "lookup_invoice");
    }

    #[test]
    fn lookup_invoice_result_accessor_reads_preimage() {
        let r = response(
            "lookup_invoice",
            None,
            json!({ "payment_hash": "ph01", "state": "settled", "preimage": "pre01" }),
        );
        let res = r.lookup_invoice_result().expect("must decode");
        assert_eq!(res.preimage.as_deref(), Some("pre01"));
        assert_eq!(res.payment_hash, "ph01");
    }

    #[test]
    fn lookup_invoice_result_accessor_reads_state() {
        let r = response(
            "lookup_invoice",
            None,
            json!({ "payment_hash": "ph02", "state": "settled" }),
        );
        let res = r.lookup_invoice_result().expect("must decode");
        assert_eq!(res.state.as_deref(), Some("settled"));
        assert_eq!(res.preimage, None);
    }

    /// A `lookup_invoice` accessor on a `pay_invoice` response must return None.
    #[test]
    fn lookup_invoice_result_wrong_result_type_is_none() {
        let r = response(
            "pay_invoice",
            None,
            json!({ "payment_hash": "x", "state": "settled" }),
        );
        assert!(r.lookup_invoice_result().is_none());
    }

    /// An error response must never surface a usable lookup result.
    #[test]
    fn lookup_invoice_result_with_error_is_none() {
        let err = NwcError {
            code: "NOT_FOUND".into(),
            message: "no such invoice".into(),
        };
        let r = response(
            "lookup_invoice",
            Some(err),
            json!({ "payment_hash": "x", "state": "settled" }),
        );
        assert!(r.lookup_invoice_result().is_none());
    }

    fn response(
        result_type: &str,
        error: Option<NwcError>,
        result: serde_json::Value,
    ) -> NwcResponse {
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
        let err = NwcError {
            code: "INTERNAL".into(),
            message: "boom".into(),
        };
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
