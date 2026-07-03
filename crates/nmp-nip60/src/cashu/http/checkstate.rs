//! `/v1/checkstate` (NUT-07) — request construction and response validation.

use super::{
    parse_json_response, MintHttpMethod, MintHttpOperation, MintHttpRequest, MintRawResponse,
};
use crate::cashu::crypto::hash_to_curve;
use crate::cashu::types::{ProofState, StateCheckRequest, StateCheckResponse};
use crate::error::Nip60Error;

/// Compute each secret's `Y = hash_to_curve(secret)` in the same order as
/// `secrets`, build the `POST /v1/checkstate` request, and return the
/// expected `Y` order so [`parse_check_state_response`] can catch a mint
/// that reorders (or drops) entries in its reply.
///
/// `Y` is always computed from the secret STRING's raw UTF-8 bytes
/// (`secret.as_bytes()`) — matching exactly how [`super::blinded`] blinds a
/// secret at mint/swap time (`blind_message(secret.as_bytes(), ..)`), for
/// both a random hex-encoded secret and a NUT-11 P2PK JSON secret
/// (`["P2PK", {...}]`, which does not even start with `{`). A prior
/// implementation hex-decoded non-`{`-prefixed secrets before hashing,
/// which silently computed the wrong `Y` for ordinary proofs (and would
/// have failed outright on a P2PK secret, since a JSON array string is not
/// valid hex) — this is corrected here.
pub fn build_check_state_request(
    secrets: &[String],
) -> Result<(MintHttpRequest, Vec<String>), Nip60Error> {
    if secrets.is_empty() {
        return Err(Nip60Error::Invalid(
            "check-state requires at least one secret".into(),
        ));
    }
    let ys: Vec<String> = secrets
        .iter()
        .map(|s| {
            let pt = hash_to_curve(s.as_bytes())?;
            Ok::<String, Nip60Error>(hex::encode(pt.serialize()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let body = serde_json::to_vec(&StateCheckRequest { ys: ys.clone() })?;
    Ok((
        MintHttpRequest {
            operation: MintHttpOperation::CheckState,
            method: MintHttpMethod::Post,
            path: "/v1/checkstate".to_string(),
            body,
        },
        ys,
    ))
}

/// Validate + decode a check-state response. Rejects a response whose
/// length or `Y` ordering does not exactly match `expected_ys` — either
/// would silently mis-attribute one proof's state to a different secret.
pub fn parse_check_state_response(
    raw: &MintRawResponse,
    expected_ys: &[String],
) -> Result<Vec<ProofState>, Nip60Error> {
    let resp: StateCheckResponse = parse_json_response(raw, "check-state response")?;
    if resp.states.len() != expected_ys.len() {
        return Err(Nip60Error::MintProtocol(format!(
            "mint returned {} proof states, expected {}",
            resp.states.len(),
            expected_ys.len()
        )));
    }
    for (i, (state, expected_y)) in resp.states.iter().zip(expected_ys.iter()).enumerate() {
        if &state.y != expected_y {
            return Err(Nip60Error::MintProtocol(format!(
                "check-state response #{i} carries Y for a different proof than requested"
            )));
        }
    }
    Ok(resp.states)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cashu::http::mint_http_support::ok;
    use crate::cashu::types::ProofSpendState;

    #[test]
    fn checkstate_rejects_non_json() {
        let raw = MintRawResponse {
            status_code: 200,
            body: b"not json".to_vec(),
        };
        let err = parse_check_state_response(&raw, &["y1".to_string()]).unwrap_err();
        assert!(matches!(err, Nip60Error::MintProtocol(_)));
    }

    #[test]
    fn checkstate_rejects_length_mismatch() {
        let raw = ok(br#"{"states":[]}"#);
        let err = parse_check_state_response(&raw, &["y1".to_string()]).unwrap_err();
        assert!(matches!(err, Nip60Error::MintProtocol(_)));
    }

    #[test]
    fn checkstate_rejects_y_order_mismatch() {
        let raw = ok(br#"{"states":[{"Y":"other-y","state":"UNSPENT"}]}"#);
        let err = parse_check_state_response(&raw, &["expected-y".to_string()]).unwrap_err();
        assert!(matches!(err, Nip60Error::MintProtocol(_)));
    }

    #[test]
    fn checkstate_rejects_unknown_state() {
        let raw = ok(br#"{"states":[{"Y":"y1","state":"WEIRD"}]}"#);
        assert!(parse_check_state_response(&raw, &["y1".to_string()]).is_err());
    }

    #[test]
    fn checkstate_accepts_matching_order() {
        let raw = ok(br#"{"states":[{"Y":"y1","state":"UNSPENT"},{"Y":"y2","state":"SPENT"}]}"#);
        let states =
            parse_check_state_response(&raw, &["y1".to_string(), "y2".to_string()]).unwrap();
        assert_eq!(states[0].state, ProofSpendState::Unspent);
        assert_eq!(states[1].state, ProofSpendState::Spent);
    }

    /// Regression: a plain hex-random secret and a NUT-11 P2PK JSON secret
    /// must both hash from their raw string bytes — the request builder
    /// must not special-case `{`-prefixed vs. hex-looking secrets (see the
    /// module-level doc comment on [`build_check_state_request`]).
    #[test]
    fn build_check_state_request_hashes_hex_and_p2pk_secrets_consistently() {
        let hex_secret = hex::encode(crate::cashu::crypto::random_secret());
        let p2pk_secret = crate::nutzap::p2pk_secret("02".to_string().repeat(33).as_str());
        assert!(
            p2pk_secret.starts_with('['),
            "P2PK secret is a JSON array, not object"
        );

        let (_, ys) =
            build_check_state_request(&[hex_secret.clone(), p2pk_secret.clone()]).unwrap();

        let expected_hex_y = hex::encode(hash_to_curve(hex_secret.as_bytes()).unwrap().serialize());
        let expected_p2pk_y =
            hex::encode(hash_to_curve(p2pk_secret.as_bytes()).unwrap().serialize());
        assert_eq!(ys, vec![expected_hex_y, expected_p2pk_y]);
    }
}
