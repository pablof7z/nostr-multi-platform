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
