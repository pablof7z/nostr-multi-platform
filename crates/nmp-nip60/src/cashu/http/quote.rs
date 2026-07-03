//! Mint-quote request construction and response validation (NUT-04/NUT-23).

use super::{
    parse_json_response, MintHttpMethod, MintHttpOperation, MintHttpRequest, MintRawResponse,
};
use crate::cashu::types::{MintQuoteRequest, MintQuoteResponse};
use crate::error::Nip60Error;

pub fn build_mint_quote_bolt11_request(
    amount_sats: u64,
    unit: &str,
) -> Result<MintHttpRequest, Nip60Error> {
    if amount_sats == 0 {
        return Err(Nip60Error::Invalid(
            "mint quote amount must be greater than zero".into(),
        ));
    }
    let body = serde_json::to_vec(&MintQuoteRequest {
        amount: amount_sats,
        unit: unit.to_string(),
    })?;
    Ok(MintHttpRequest {
        operation: MintHttpOperation::CreateMintQuoteBolt11,
        method: MintHttpMethod::Post,
        path: "/v1/mint/quote/bolt11".to_string(),
        body,
    })
}

pub fn build_get_mint_quote_bolt11_request(quote_id: &str) -> Result<MintHttpRequest, Nip60Error> {
    if quote_id.trim().is_empty() {
        return Err(Nip60Error::Invalid(
            "mint quote id must not be empty".into(),
        ));
    }
    Ok(MintHttpRequest {
        operation: MintHttpOperation::GetMintQuoteBolt11,
        method: MintHttpMethod::Get,
        path: format!("/v1/mint/quote/bolt11/{quote_id}"),
        body: Vec::new(),
    })
}

/// What a mint-quote response is expected to echo back. Every field is
/// optional because the two call sites know different things: creating a
/// quote knows the amount/unit it just requested; polling an existing
/// quote's status only knows the quote id.
#[derive(Clone, Copy, Debug, Default)]
pub struct MintQuoteExpectation<'a> {
    pub amount: Option<u64>,
    pub unit: Option<&'a str>,
    /// The quote id this response should belong to — set when polling an
    /// existing quote's status. A mismatch here would mean a caller (or a
    /// misbehaving transport) fed back the wrong quote's response.
    pub quote_id: Option<&'a str>,
}

/// Validate + decode a mint-quote response against `expect`.
pub fn parse_mint_quote_bolt11_response(
    raw: &MintRawResponse,
    expect: MintQuoteExpectation<'_>,
) -> Result<MintQuoteResponse, Nip60Error> {
    let resp: MintQuoteResponse = parse_json_response(raw, "mint quote")?;
    if resp.quote.trim().is_empty() {
        return Err(Nip60Error::MintProtocol(
            "mint quote response carries an empty quote id".into(),
        ));
    }
    if let Some(expected_quote_id) = expect.quote_id {
        if resp.quote != expected_quote_id {
            return Err(Nip60Error::MintProtocol(
                "mint quote response carries a different quote id than requested".into(),
            ));
        }
    }
    if let Some(expected_amount) = expect.amount {
        if resp.amount != expected_amount {
            return Err(Nip60Error::MintProtocol(format!(
                "mint quote amount mismatch: requested {expected_amount}, mint returned {}",
                resp.amount
            )));
        }
    }
    if let Some(expected_unit) = expect.unit {
        if resp.unit != expected_unit {
            return Err(Nip60Error::MintProtocol(format!(
                "mint quote unit mismatch: requested {expected_unit}, mint returned {}",
                resp.unit
            )));
        }
    }
    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cashu::http::mint_http_support::ok;
    use crate::cashu::types::MintQuoteState;

    #[test]
    fn mint_quote_rejects_html_error_page() {
        let raw = crate::cashu::http::MintRawResponse {
            status_code: 502,
            body: b"<html><body>Bad Gateway</body></html>".to_vec(),
        };
        let err = parse_mint_quote_bolt11_response(
            &raw,
            MintQuoteExpectation {
                amount: Some(10),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(matches!(err, Nip60Error::MintHttp(_)));
    }

    #[test]
    fn mint_quote_rejects_amount_mismatch() {
        let raw =
            ok(br#"{"quote":"q1","request":"lnbc1","amount":5,"unit":"sat","state":"UNPAID"}"#);
        let err = parse_mint_quote_bolt11_response(
            &raw,
            MintQuoteExpectation {
                amount: Some(10),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(matches!(err, Nip60Error::MintProtocol(_)));
    }

    #[test]
    fn mint_quote_rejects_unit_mismatch() {
        let raw =
            ok(br#"{"quote":"q1","request":"lnbc1","amount":10,"unit":"msat","state":"UNPAID"}"#);
        let err = parse_mint_quote_bolt11_response(
            &raw,
            MintQuoteExpectation {
                unit: Some("sat"),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(matches!(err, Nip60Error::MintProtocol(_)));
    }

    #[test]
    fn mint_quote_status_rejects_quote_id_mismatch() {
        let raw = ok(br#"{"quote":"other-quote","request":"lnbc1","amount":10,"unit":"sat","state":"UNPAID"}"#);
        let err = parse_mint_quote_bolt11_response(
            &raw,
            MintQuoteExpectation {
                quote_id: Some("expected-quote"),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(matches!(err, Nip60Error::MintProtocol(_)));
    }

    #[test]
    fn mint_quote_rejects_empty_or_invalid_quote_id() {
        let raw =
            ok(br#"{"quote":"","request":"lnbc1","amount":10,"unit":"sat","state":"UNPAID"}"#);
        let err = parse_mint_quote_bolt11_response(
            &raw,
            MintQuoteExpectation {
                amount: Some(10),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(matches!(err, Nip60Error::MintProtocol(_)));
    }

    #[test]
    fn mint_quote_rejects_unknown_state() {
        let raw =
            ok(br#"{"quote":"q1","request":"lnbc1","amount":10,"unit":"sat","state":"WEIRD"}"#);
        assert!(parse_mint_quote_bolt11_response(
            &raw,
            MintQuoteExpectation {
                amount: Some(10),
                ..Default::default()
            }
        )
        .is_err());
    }

    #[test]
    fn mint_quote_accepts_matching_amount_and_unit() {
        let raw =
            ok(br#"{"quote":"q1","request":"lnbc1","amount":10,"unit":"sat","state":"UNPAID"}"#);
        let resp = parse_mint_quote_bolt11_response(
            &raw,
            MintQuoteExpectation {
                amount: Some(10),
                unit: Some("sat"),
                ..Default::default()
            },
        )
        .expect("should parse");
        assert_eq!(resp.state, MintQuoteState::Unpaid);
    }
}
