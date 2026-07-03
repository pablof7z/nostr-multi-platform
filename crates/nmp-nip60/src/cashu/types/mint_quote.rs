//! NUT-04 mint-quote request/response types.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::REDACTED;

/// Request body for `POST /v1/mint/quote/bolt11`.
#[derive(Debug, Serialize)]
pub struct MintQuoteRequest {
    pub amount: u64,
    pub unit: String,
}

/// NUT-04 mint-quote lifecycle state. A typed enum (rather than the raw wire
/// string) means a state the mint invents that we don't recognize fails
/// `serde` deserialization closed instead of silently comparing false to
/// every known branch (fail-closed on an unrecognized mint response).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum MintQuoteState {
    Unpaid,
    Paid,
    Issued,
}

/// Response from `POST /v1/mint/quote/bolt11` and
/// `GET /v1/mint/quote/bolt11/{quote_id}`.
#[derive(Clone, Deserialize)]
pub struct MintQuoteResponse {
    pub quote: String,
    pub request: String, // bolt11 invoice
    pub amount: u64,
    pub unit: String,
    pub state: MintQuoteState,
    /// Expiry timestamp. Some mints return `null` here.
    #[serde(default)]
    pub expiry: Option<u64>,
}

impl fmt::Debug for MintQuoteResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MintQuoteResponse")
            .field("quote", &REDACTED)
            .field("request", &REDACTED)
            .field("amount", &self.amount)
            .field("unit", &self.unit)
            .field("state", &self.state)
            .field("expiry", &self.expiry)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mint_quote_response_rejects_unknown_state() {
        let json = r#"{"quote":"q","request":"lnbc1","amount":1,"unit":"sat","state":"WEIRD"}"#;
        assert!(serde_json::from_str::<MintQuoteResponse>(json).is_err());
    }

    #[test]
    fn sensitive_debug_redacts_quote_and_request() {
        let quote = MintQuoteResponse {
            quote: "super-secret-quote-id".into(),
            request: "lnbc1secretinvoice".into(),
            amount: 10,
            unit: "sat".into(),
            state: MintQuoteState::Unpaid,
            expiry: None,
        };
        let debug = format!("{quote:?}");
        assert!(!debug.contains("super-secret-quote-id"));
        assert!(!debug.contains("lnbc1secretinvoice"));
    }
}
