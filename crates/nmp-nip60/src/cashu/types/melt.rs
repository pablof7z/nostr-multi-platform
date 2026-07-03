//! NUT-05 melt request/response types.
//!
//! Melt (redeem ecash for a Lightning payment) is NOT wired to any wallet
//! flow yet — the epic (#2864) explicitly defers "Cashu melt → pay_bolt11"
//! until NUT-05 + double-payment reconciliation are proven. These are the
//! request/response shapes a later workstream needs; they are exercised by
//! round-trip tests below so they are not unverified dead code.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::minting::{BlindSignature, BlindedMessage};
use super::proof::Proof;
use super::REDACTED;

/// NUT-05 melt-quote lifecycle state. Distinct from
/// [`super::mint_quote::MintQuoteState`] — a melt quote's terminal state is
/// `Paid`, but it also has a `Pending` state (an in-flight Lightning
/// payment) that a mint quote never reaches.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum MeltQuoteState {
    Unpaid,
    Pending,
    Paid,
}

/// NUT-23 amountless-bolt11 melt option — pay an invoice that carries no
/// amount by specifying it out of band.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AmountlessBolt11 {
    pub amount_msat: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MeltQuoteBolt11Options {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amountless: Option<AmountlessBolt11>,
}

/// Request body for `POST /v1/melt/quote/bolt11`.
#[derive(Clone, Serialize)]
pub struct MeltQuoteBolt11Request {
    /// The bolt11 invoice to pay.
    pub request: String,
    pub unit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<MeltQuoteBolt11Options>,
}

impl fmt::Debug for MeltQuoteBolt11Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MeltQuoteBolt11Request")
            .field("request", &REDACTED)
            .field("unit", &self.unit)
            .field("options", &self.options)
            .finish()
    }
}

/// Response from `POST /v1/melt/quote/bolt11` and
/// `GET /v1/melt/quote/bolt11/{quote_id}`.
#[derive(Clone, Deserialize)]
pub struct MeltQuoteBolt11Response {
    pub quote: String,
    pub request: String,
    pub amount: u64,
    pub unit: String,
    pub fee_reserve: u64,
    pub state: MeltQuoteState,
    #[serde(default)]
    pub expiry: Option<u64>,
    /// Populated once the mint has paid the invoice — the Lightning payment
    /// preimage. Secret: proves payment; must never be logged.
    #[serde(default)]
    pub payment_preimage: Option<String>,
}

impl fmt::Debug for MeltQuoteBolt11Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MeltQuoteBolt11Response")
            .field("quote", &REDACTED)
            .field("request", &REDACTED)
            .field("amount", &self.amount)
            .field("unit", &self.unit)
            .field("fee_reserve", &self.fee_reserve)
            .field("state", &self.state)
            .field("expiry", &self.expiry)
            .field(
                "payment_preimage",
                &self.payment_preimage.as_ref().map(|_| REDACTED),
            )
            .finish()
    }
}

/// Request body for `POST /v1/melt/bolt11`.
#[derive(Clone, Serialize)]
pub struct MeltBolt11Request {
    pub quote: String,
    pub inputs: Vec<Proof>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outputs: Option<Vec<BlindedMessage>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefer_async: Option<bool>,
}

impl fmt::Debug for MeltBolt11Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MeltBolt11Request")
            .field("quote", &REDACTED)
            .field("input_count", &self.inputs.len())
            .field(
                "output_count",
                &self.outputs.as_ref().map(Vec::len).unwrap_or(0),
            )
            .field("prefer_async", &self.prefer_async)
            .finish()
    }
}

/// Response from `POST /v1/melt/bolt11`. Carries the same terminal-status
/// shape as [`MeltQuoteBolt11Response`] plus NUT-08 `change` — blind
/// signatures for any overpaid fee-reserve amount the mint returns as fresh
/// proofs.
#[derive(Clone, Deserialize)]
pub struct MeltBolt11Response {
    pub quote: String,
    pub request: String,
    pub amount: u64,
    pub unit: String,
    pub fee_reserve: u64,
    pub state: MeltQuoteState,
    #[serde(default)]
    pub expiry: Option<u64>,
    #[serde(default)]
    pub payment_preimage: Option<String>,
    #[serde(default)]
    pub change: Vec<BlindSignature>,
}

impl fmt::Debug for MeltBolt11Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MeltBolt11Response")
            .field("quote", &REDACTED)
            .field("request", &REDACTED)
            .field("amount", &self.amount)
            .field("unit", &self.unit)
            .field("fee_reserve", &self.fee_reserve)
            .field("state", &self.state)
            .field("expiry", &self.expiry)
            .field(
                "payment_preimage",
                &self.payment_preimage.as_ref().map(|_| REDACTED),
            )
            .field("change_count", &self.change.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn melt_quote_bolt11_request_round_trips_amountless_options() {
        let req = MeltQuoteBolt11Request {
            request: "lnbc1...".into(),
            unit: "sat".into(),
            options: Some(MeltQuoteBolt11Options {
                amountless: Some(AmountlessBolt11 { amount_msat: 1234 }),
            }),
        };
        let json = serde_json::to_string(&req).expect("serialize");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(value["options"]["amountless"]["amount_msat"], 1234);
    }

    #[test]
    fn melt_quote_bolt11_response_round_trips() {
        let json = r#"{
            "quote": "quote-id-1",
            "request": "lnbc1...",
            "amount": 1000,
            "unit": "sat",
            "fee_reserve": 5,
            "state": "PENDING",
            "expiry": 1700000000,
            "payment_preimage": null
        }"#;
        let resp: MeltQuoteBolt11Response = serde_json::from_str(json).expect("parse");
        assert_eq!(resp.state, MeltQuoteState::Pending);
        assert_eq!(resp.amount, 1000);
        assert_eq!(resp.fee_reserve, 5);
        assert!(resp.payment_preimage.is_none());
    }

    #[test]
    fn melt_bolt11_request_round_trips_outputs_and_prefer_async() {
        let req = MeltBolt11Request {
            quote: "quote-id-2".into(),
            inputs: vec![],
            outputs: Some(vec![BlindedMessage {
                amount: 4,
                id: "00abc".into(),
                b_prime: "02aa".into(),
            }]),
            prefer_async: Some(true),
        };
        let json = serde_json::to_string(&req).expect("serialize");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(value["quote"], "quote-id-2");
        assert_eq!(value["outputs"][0]["amount"], 4);
        assert_eq!(value["prefer_async"], true);
    }

    #[test]
    fn melt_bolt11_response_round_trips_change_signatures() {
        let json = r#"{
            "quote": "quote-id-3",
            "request": "lnbc1...",
            "amount": 1000,
            "unit": "sat",
            "fee_reserve": 5,
            "state": "PAID",
            "payment_preimage": "abcd1234",
            "change": [
                {"amount": 2, "id": "00abc", "C_": "02bb"}
            ]
        }"#;
        let resp: MeltBolt11Response = serde_json::from_str(json).expect("parse");
        assert_eq!(resp.state, MeltQuoteState::Paid);
        assert_eq!(resp.change.len(), 1);
        assert_eq!(resp.change[0].amount, 2);
        assert_eq!(resp.payment_preimage.as_deref(), Some("abcd1234"));
    }

    #[test]
    fn sensitive_debug_redacts_quote_and_preimage() {
        let melt_resp = MeltBolt11Response {
            quote: "melt-quote-id".into(),
            request: "lnbc1...".into(),
            amount: 10,
            unit: "sat".into(),
            fee_reserve: 1,
            state: MeltQuoteState::Paid,
            expiry: None,
            payment_preimage: Some("top-secret-preimage".into()),
            change: vec![],
        };
        let debug = format!("{melt_resp:?}");
        assert!(!debug.contains("melt-quote-id"));
        assert!(!debug.contains("top-secret-preimage"));
    }
}
