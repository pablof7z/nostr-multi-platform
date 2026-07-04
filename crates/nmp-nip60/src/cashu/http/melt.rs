//! `/v1/melt/quote/bolt11` + `/v1/melt/bolt11` (NUT-05) — request
//! construction and response validation for redeeming ecash as a Lightning
//! payment. This is the SOURCE-mint leg of a cross-mint transfer
//! (`nmp-wallet`'s `CrossMintTransfer` saga, #3003): melt proofs here to pay
//! a bolt11 mint-quote obtained from a different (target) mint.
//!
//! Mirrors [`super::mint`]/[`super::quote`]'s prepare/finalize shape, with
//! one addition: NUT-08 blank change outputs. The mint may return a fee
//! reserve payment (`melt_quote.amount + melt_quote.fee_reserve` was
//! reserved, but the real Lightning routing fee is usually lower) as fresh
//! blind-signed proofs — see [`super::blinded::build_blank_outputs`] /
//! [`super::blinded::finalize_blank_outputs`].

use std::fmt;

use nostr::secp256k1::{All, Secp256k1};

use super::blinded::{blank_output_count, build_blank_outputs, finalize_blank_outputs};
use super::{
    parse_json_response, DleqPolicy, MintHttpMethod, MintHttpOperation, MintHttpRequest,
    MintRawResponse, REDACTED,
};
use crate::cashu::types::{
    KeySet, MeltBolt11Request, MeltBolt11Response, MeltQuoteBolt11Request, MeltQuoteBolt11Response,
    Proof,
};
use crate::error::Nip60Error;

use super::blinded::BlindedOutputSet;

// ─── Melt quote (NUT-05) ────────────────────────────────────────────────────

pub fn build_melt_quote_bolt11_request(
    bolt11: &str,
    unit: &str,
) -> Result<MintHttpRequest, Nip60Error> {
    if bolt11.trim().is_empty() {
        return Err(Nip60Error::Invalid(
            "melt quote bolt11 invoice must not be empty".into(),
        ));
    }
    let body = serde_json::to_vec(&MeltQuoteBolt11Request {
        request: bolt11.to_string(),
        unit: unit.to_string(),
        options: None,
    })?;
    Ok(MintHttpRequest {
        operation: MintHttpOperation::CreateMeltQuoteBolt11,
        method: MintHttpMethod::Post,
        path: "/v1/melt/quote/bolt11".to_string(),
        body,
    })
}

pub fn build_get_melt_quote_bolt11_request(quote_id: &str) -> Result<MintHttpRequest, Nip60Error> {
    if quote_id.trim().is_empty() {
        return Err(Nip60Error::Invalid(
            "melt quote id must not be empty".into(),
        ));
    }
    Ok(MintHttpRequest {
        operation: MintHttpOperation::GetMeltQuoteBolt11,
        method: MintHttpMethod::Get,
        path: format!("/v1/melt/quote/bolt11/{quote_id}"),
        body: Vec::new(),
    })
}

/// What a melt-quote response is expected to echo back. Mirrors
/// [`super::quote::MintQuoteExpectation`] — `quote_id` is set when polling
/// an existing quote's status, so a mismatch (a caller, or a misbehaving
/// transport, feeding back the wrong quote's response) is caught.
#[derive(Clone, Copy, Debug, Default)]
pub struct MeltQuoteExpectation<'a> {
    pub quote_id: Option<&'a str>,
}

pub fn parse_melt_quote_bolt11_response(
    raw: &MintRawResponse,
    expect: MeltQuoteExpectation<'_>,
) -> Result<MeltQuoteBolt11Response, Nip60Error> {
    let resp: MeltQuoteBolt11Response = parse_json_response(raw, "melt quote")?;
    if resp.quote.trim().is_empty() {
        return Err(Nip60Error::MintProtocol(
            "melt quote response carries an empty quote id".into(),
        ));
    }
    if let Some(expected_quote_id) = expect.quote_id {
        if resp.quote != expected_quote_id {
            return Err(Nip60Error::MintProtocol(
                "melt quote response carries a different quote id than requested".into(),
            ));
        }
    }
    Ok(resp)
}

// ─── Melt (NUT-05) ──────────────────────────────────────────────────────────

/// Everything needed to construct the `POST /v1/melt/bolt11` request and
/// later unblind any NUT-08 change in its response.
pub struct PreparedMeltBolt11Request {
    pub http: MintHttpRequest,
    quote: String,
    keyset: KeySet,
    /// `None` when the melt quote's `fee_reserve` is `0` — no change is
    /// possible, so no blank outputs are sent.
    change_outputs: Option<BlindedOutputSet>,
}

impl fmt::Debug for PreparedMeltBolt11Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedMeltBolt11Request")
            .field("quote", &REDACTED)
            .field("keyset_id", &self.keyset.id)
            .field("has_change_outputs", &self.change_outputs.is_some())
            .finish()
    }
}

impl PreparedMeltBolt11Request {
    /// The melt quote id this request settles — needed by a caller (e.g.
    /// the `nmp-wallet` cross-mint journal) to correlate the eventual result
    /// back to the pending operation it started.
    #[must_use]
    pub fn quote_id(&self) -> &str {
        &self.quote
    }
}

/// Prepare a `POST /v1/melt/bolt11` request. `inputs` MUST already have been
/// durably recorded as consumed (journaled) by the caller BEFORE this
/// request is sent over the wire — melt is the irreversible leg of a
/// cross-mint transfer (the Lightning payment leaves the source mint) and
/// must never be attempted without a preceding journal write of exactly
/// which proofs are being spent.
///
/// `fee_reserve` sizes the NUT-08 blank change outputs (see
/// [`blank_output_count`]) — pass the `fee_reserve` from the
/// [`MeltQuoteBolt11Response`] this melt is settling, NOT the sum of
/// `inputs`.
pub fn prepare_melt_bolt11_request(
    quote_id: &str,
    fee_reserve: u64,
    inputs: Vec<Proof>,
    keyset: &KeySet,
    secp: &Secp256k1<All>,
) -> Result<PreparedMeltBolt11Request, Nip60Error> {
    if quote_id.trim().is_empty() {
        return Err(Nip60Error::Invalid(
            "melt quote id must not be empty".into(),
        ));
    }
    if inputs.is_empty() {
        return Err(Nip60Error::Invalid(
            "melt requires at least one input proof".into(),
        ));
    }

    let count = blank_output_count(fee_reserve);
    let change_outputs = if count == 0 {
        None
    } else {
        Some(build_blank_outputs(count, &keyset.id, secp)?)
    };

    let outputs_wire = change_outputs.as_ref().map(|o| o.wire.clone());
    let body = serde_json::to_vec(&MeltBolt11Request {
        quote: quote_id.to_string(),
        inputs,
        outputs: outputs_wire,
        prefer_async: None,
    })?;

    Ok(PreparedMeltBolt11Request {
        http: MintHttpRequest {
            operation: MintHttpOperation::MeltBolt11,
            method: MintHttpMethod::Post,
            path: "/v1/melt/bolt11".to_string(),
            body,
        },
        quote: quote_id.to_string(),
        keyset: keyset.clone(),
        change_outputs,
    })
}

/// Validate + decode a `POST /v1/melt/bolt11` response, unblinding any
/// NUT-08 change into spendable [`Proof`]s. Ambiguous responses (a
/// transport failure with no HTTP response at all) are NOT handled here —
/// this function only ever sees a response that actually arrived; the
/// caller (`nmp-wallet`'s cross-mint saga) is responsible for treating a
/// transport-level failure as `Unknown` and reconciling via
/// `get_melt_quote_status` rather than assuming success or failure.
pub fn finalize_melt_bolt11_response(
    prepared: &PreparedMeltBolt11Request,
    raw: &MintRawResponse,
    dleq_policy: DleqPolicy,
    secp: &Secp256k1<All>,
) -> Result<(MeltBolt11Response, Vec<Proof>), Nip60Error> {
    let resp: MeltBolt11Response = parse_json_response(raw, "melt response")?;
    if resp.quote != prepared.quote {
        return Err(Nip60Error::MintProtocol(
            "melt response carries a different quote id than requested".into(),
        ));
    }
    let change = match (&prepared.change_outputs, resp.change.is_empty()) {
        (_, true) => Vec::new(),
        (Some(outputs), false) => {
            finalize_blank_outputs(outputs, &prepared.keyset, &resp.change, dleq_policy, secp)?
        }
        (None, false) => {
            return Err(Nip60Error::MintProtocol(
                "melt response carries change but no blank outputs were offered".into(),
            ));
        }
    };
    Ok((resp, change))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cashu::crypto::random_secret;
    use crate::cashu::http::mint_http_support::{fixture_keyset, ok, secp};
    use crate::cashu::types::{BlindSignature, MeltQuoteState, Proof as ProofType};

    fn fixture_input(keyset_id: &str) -> ProofType {
        ProofType {
            amount: 8,
            id: keyset_id.to_string(),
            secret: hex::encode(random_secret()),
            c: "02".to_string() + &"aa".repeat(32),
            dleq: None,
            witness: None,
        }
    }

    #[test]
    fn melt_quote_rejects_empty_invoice() {
        let err = build_melt_quote_bolt11_request("", "sat").unwrap_err();
        assert!(matches!(err, Nip60Error::Invalid(_)));
    }

    #[test]
    fn melt_quote_rejects_html_error_page() {
        let raw = MintRawResponse {
            status_code: 502,
            body: b"<html>Bad Gateway</html>".to_vec(),
        };
        let err =
            parse_melt_quote_bolt11_response(&raw, MeltQuoteExpectation::default()).unwrap_err();
        assert!(matches!(err, Nip60Error::MintHttp(_)));
    }

    #[test]
    fn melt_quote_rejects_quote_id_mismatch() {
        let raw = ok(br#"{"quote":"other","request":"lnbc1","amount":10,"unit":"sat","fee_reserve":1,"state":"UNPAID"}"#);
        let err = parse_melt_quote_bolt11_response(
            &raw,
            MeltQuoteExpectation {
                quote_id: Some("expected"),
            },
        )
        .unwrap_err();
        assert!(matches!(err, Nip60Error::MintProtocol(_)));
    }

    #[test]
    fn melt_quote_rejects_empty_quote_id() {
        let raw = ok(br#"{"quote":"","request":"lnbc1","amount":10,"unit":"sat","fee_reserve":1,"state":"UNPAID"}"#);
        let err =
            parse_melt_quote_bolt11_response(&raw, MeltQuoteExpectation::default()).unwrap_err();
        assert!(matches!(err, Nip60Error::MintProtocol(_)));
    }

    #[test]
    fn melt_quote_accepts_well_formed_response() {
        let raw = ok(br#"{"quote":"q1","request":"lnbc1","amount":10,"unit":"sat","fee_reserve":2,"state":"UNPAID"}"#);
        let resp = parse_melt_quote_bolt11_response(
            &raw,
            MeltQuoteExpectation {
                quote_id: Some("q1"),
            },
        )
        .expect("should parse");
        assert_eq!(resp.state, MeltQuoteState::Unpaid);
        assert_eq!(resp.fee_reserve, 2);
    }

    #[test]
    fn prepare_melt_rejects_empty_inputs() {
        let (keyset, _) = fixture_keyset();
        let secp = secp();
        let err = prepare_melt_bolt11_request("q1", 2, vec![], &keyset, &secp).unwrap_err();
        assert!(matches!(err, Nip60Error::Invalid(_)));
    }

    #[test]
    fn prepare_melt_omits_outputs_when_fee_reserve_is_zero() {
        let (keyset, _) = fixture_keyset();
        let secp = secp();
        let prepared =
            prepare_melt_bolt11_request("q1", 0, vec![fixture_input(&keyset.id)], &keyset, &secp)
                .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&prepared.http.body).unwrap();
        assert!(value.get("outputs").is_none() || value["outputs"].is_null());
    }

    #[test]
    fn prepare_melt_includes_blank_outputs_when_fee_reserve_positive() {
        let (keyset, _) = fixture_keyset();
        let secp = secp();
        let prepared =
            prepare_melt_bolt11_request("q1", 4, vec![fixture_input(&keyset.id)], &keyset, &secp)
                .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&prepared.http.body).unwrap();
        assert_eq!(value["outputs"].as_array().unwrap().len(), 2); // blank_output_count(4) == 2
    }

    #[test]
    fn finalize_melt_rejects_quote_id_mismatch() {
        let (keyset, _) = fixture_keyset();
        let secp = secp();
        let prepared =
            prepare_melt_bolt11_request("q1", 0, vec![fixture_input(&keyset.id)], &keyset, &secp)
                .unwrap();
        let raw = ok(br#"{"quote":"other-quote","request":"lnbc1","amount":8,"unit":"sat","fee_reserve":0,"state":"PAID"}"#);
        let err =
            finalize_melt_bolt11_response(&prepared, &raw, DleqPolicy::VerifyIfPresent, &secp)
                .unwrap_err();
        assert!(matches!(err, Nip60Error::MintProtocol(_)));
    }

    #[test]
    fn finalize_melt_rejects_change_without_blank_outputs_offered() {
        let (keyset, mint_sk) = fixture_keyset();
        let secp = secp();
        let prepared =
            prepare_melt_bolt11_request("q1", 0, vec![fixture_input(&keyset.id)], &keyset, &secp)
                .unwrap();
        // Simulate a mint that returns change even though no blank outputs
        // were offered — must be rejected, not silently ignored or
        // (worse) treated as free money.
        let bogus_change = BlindSignature {
            amount: 1,
            id: keyset.id.clone(),
            c_prime: {
                let sk = mint_sk;
                let pk = nostr::secp256k1::PublicKey::from_secret_key(&secp, &sk);
                hex::encode(pk.serialize())
            },
            dleq: None,
        };
        let raw_body = serde_json::to_vec(&serde_json::json!({
            "quote": "q1",
            "request": "lnbc1",
            "amount": 8,
            "unit": "sat",
            "fee_reserve": 0,
            "state": "PAID",
            "change": [bogus_change],
        }))
        .unwrap();
        let raw = ok(&raw_body);
        let err =
            finalize_melt_bolt11_response(&prepared, &raw, DleqPolicy::VerifyIfPresent, &secp)
                .unwrap_err();
        assert!(matches!(err, Nip60Error::MintProtocol(_)));
    }

    #[test]
    fn finalize_melt_accepts_no_change_when_fee_reserve_zero() {
        let (keyset, _) = fixture_keyset();
        let secp = secp();
        let prepared =
            prepare_melt_bolt11_request("q1", 0, vec![fixture_input(&keyset.id)], &keyset, &secp)
                .unwrap();
        let raw = ok(
            br#"{"quote":"q1","request":"lnbc1","amount":8,"unit":"sat","fee_reserve":0,"state":"PAID","payment_preimage":"pre"}"#,
        );
        let (resp, change) =
            finalize_melt_bolt11_response(&prepared, &raw, DleqPolicy::VerifyIfPresent, &secp)
                .expect("no change is a valid response");
        assert_eq!(resp.state, MeltQuoteState::Paid);
        assert!(change.is_empty());
    }

    #[test]
    fn finalize_melt_unblinds_change_signatures() {
        let (keyset, mint_sk) = fixture_keyset();
        let secp = secp();
        let prepared =
            prepare_melt_bolt11_request("q1", 2, vec![fixture_input(&keyset.id)], &keyset, &secp)
                .unwrap();
        let change_outputs = prepared.change_outputs.as_ref().unwrap();
        let sig = {
            let b_prime = nostr::secp256k1::PublicKey::from_slice(
                &hex::decode(&change_outputs.b_primes_hex[0]).unwrap(),
            )
            .unwrap();
            let k_scalar = nostr::secp256k1::Scalar::from(mint_sk);
            let c_prime = b_prime.mul_tweak(&secp, &k_scalar).unwrap();
            BlindSignature {
                amount: 1,
                id: keyset.id.clone(),
                c_prime: hex::encode(c_prime.serialize()),
                dleq: None,
            }
        };
        let raw_body = serde_json::to_vec(&serde_json::json!({
            "quote": "q1",
            "request": "lnbc1",
            "amount": 8,
            "unit": "sat",
            "fee_reserve": 2,
            "state": "PAID",
            "change": [sig],
        }))
        .unwrap();
        let raw = ok(&raw_body);
        let (_, change) =
            finalize_melt_bolt11_response(&prepared, &raw, DleqPolicy::VerifyIfPresent, &secp)
                .expect("well-formed change must unblind");
        assert_eq!(change.len(), 1);
        assert_eq!(change[0].amount, 1);
    }
}
