//! Decode NIP-47 kind:23195 wallet response events.

use crate::crypto;
use crate::types::NwcResponse;

/// Attempt to decode a kind:23195 response from a raw Nostr relay message.
///
/// The relay delivers messages as JSON arrays:
/// `["EVENT", "<sub_id>", {<event_json>}]`
///
/// Returns the decrypted `NwcResponse` if the message is a kind:23195 EVENT
/// from `wallet_pubkey_hex`, otherwise returns None.
///
/// `client_secret_hex` / `wallet_pubkey_hex`: from the stored NWC connection.
#[must_use] 
pub fn try_decode_relay_message(
    relay_text: &str,
    wallet_pubkey_hex: &str,
    client_secret_hex: &str,
) -> Option<NwcResponse> {
    let outer: serde_json::Value = serde_json::from_str(relay_text).ok()?;
    let arr = outer.as_array()?;

    // ["EVENT", "<sub_id>", {event}]
    if arr.first()?.as_str()? != "EVENT" || arr.len() < 3 {
        return None;
    }
    let event = arr.get(2)?;

    let kind = event.get("kind")?.as_u64()?;
    if kind != 23195 {
        return None;
    }

    let event_pubkey = event.get("pubkey")?.as_str()?;
    if !event_pubkey.eq_ignore_ascii_case(wallet_pubkey_hex) {
        return None;
    }

    let content = event.get("content")?.as_str()?;
    let plaintext = crypto::decrypt(client_secret_hex, wallet_pubkey_hex, content).ok()?;
    serde_json::from_str::<NwcResponse>(&plaintext).ok()
}

/// Extract the event id from a relay EVENT message, alongside the decoded response.
#[must_use] 
pub fn try_decode_relay_message_with_id(
    relay_text: &str,
    wallet_pubkey_hex: &str,
    client_secret_hex: &str,
) -> Option<(String, NwcResponse)> {
    let outer: serde_json::Value = serde_json::from_str(relay_text).ok()?;
    let arr = outer.as_array()?;
    if arr.first()?.as_str()? != "EVENT" || arr.len() < 3 {
        return None;
    }
    let event = arr.get(2)?;
    let kind = event.get("kind")?.as_u64()?;
    if kind != 23195 {
        return None;
    }
    let event_pubkey = event.get("pubkey")?.as_str()?;
    if !event_pubkey.eq_ignore_ascii_case(wallet_pubkey_hex) {
        return None;
    }
    let event_id = event.get("id")?.as_str()?.to_string();
    let content = event.get("content")?.as_str()?;
    let plaintext = crypto::decrypt(client_secret_hex, wallet_pubkey_hex, content).ok()?;
    let response = serde_json::from_str::<NwcResponse>(&plaintext).ok()?;
    Some((event_id, response))
}

/// Decode a kind:23195 relay frame and return the **request** event id (the
/// kind:23194 the response is replying to) alongside the decoded response.
///
/// Per NIP-47 §3.2, the wallet service includes an `e` tag on the kind:23195
/// reply whose value is the id of the original kind:23194 request. That id —
/// not the response wrapper's own id — is the key a client uses to correlate
/// an inflight request to its response. [`try_decode_relay_message_with_id`]
/// returns the wrapper id, which is fine for log-style traceability but does
/// NOT close a per-request promise (the client never saw the wrapper id at
/// send time).
///
/// Returns `None` when the frame isn't a valid kind:23195 from
/// `wallet_pubkey_hex`, when the content fails to decrypt, when the
/// decrypted payload isn't a valid `NwcResponse`, or when the response is
/// missing an `e` tag — the last case is itself a violation of NIP-47 and
/// would leave the client unable to match the reply to any request, so we
/// fail closed (D6 — silent on unknown, never panic).
#[must_use] 
pub fn try_decode_response_for_request(
    relay_text: &str,
    wallet_pubkey_hex: &str,
    client_secret_hex: &str,
) -> Option<(String, NwcResponse)> {
    let outer: serde_json::Value = serde_json::from_str(relay_text).ok()?;
    let arr = outer.as_array()?;
    if arr.first()?.as_str()? != "EVENT" || arr.len() < 3 {
        return None;
    }
    let event = arr.get(2)?;
    let kind = event.get("kind")?.as_u64()?;
    if kind != 23195 {
        return None;
    }
    let event_pubkey = event.get("pubkey")?.as_str()?;
    if !event_pubkey.eq_ignore_ascii_case(wallet_pubkey_hex) {
        return None;
    }
    // Find the first `e` tag — NIP-47 §3.2 mandates exactly one referencing
    // the request id. `["e", "<request_id>"]` (additional positional fields
    // like a relay hint are tolerated but ignored).
    let request_event_id = event
        .get("tags")?
        .as_array()?
        .iter()
        .find_map(|t| {
            let tag = t.as_array()?;
            let name = tag.first()?.as_str()?;
            if name != "e" {
                return None;
            }
            tag.get(1)?.as_str().map(str::to_string)
        })?;
    let content = event.get("content")?.as_str()?;
    let plaintext = crypto::decrypt(client_secret_hex, wallet_pubkey_hex, content).ok()?;
    let response = serde_json::from_str::<NwcResponse>(&plaintext).ok()?;
    Some((request_event_id, response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto;
    use serde_json::json;

    // Client side of the NWC connection.
    const CLIENT_SECRET: &str =
        "0101010101010101010101010101010101010101010101010101010101010101";
    // Wallet service side — it encrypts kind:23195 responses to the client.
    const WALLET_SECRET: &str =
        "0202020202020202020202020202020202020202020202020202020202020202";

    fn wallet_pk() -> String {
        crypto::client_pubkey_hex(WALLET_SECRET).unwrap()
    }

    /// Build a realistic `["EVENT", sub, {event}]` relay frame whose `content`
    /// is the NIP-04-encrypted `response_json`, encrypted wallet→client.
    fn relay_event(kind: u64, pubkey: &str, id: &str, response_json: &serde_json::Value) -> String {
        let client_pk = crypto::client_pubkey_hex(CLIENT_SECRET).unwrap();
        let plaintext = serde_json::to_string(response_json).unwrap();
        // Wallet encrypts to the client's pubkey using the wallet secret.
        let content = crypto::encrypt(WALLET_SECRET, &client_pk, &plaintext).unwrap();
        let frame = json!([
            "EVENT",
            "sub-1",
            { "id": id, "kind": kind, "pubkey": pubkey, "content": content }
        ]);
        serde_json::to_string(&frame).unwrap()
    }

    #[test]
    fn decode_pay_invoice_success() {
        let wallet_pk = wallet_pk();
        let response = json!({
            "result_type": "pay_invoice",
            "error": null,
            "result": { "preimage": "abc123preimage" }
        });
        let frame = relay_event(23195, &wallet_pk, "evt-1", &response);
        let decoded = try_decode_relay_message(&frame, &wallet_pk, CLIENT_SECRET).unwrap();
        assert_eq!(decoded.result_type, "pay_invoice");
        assert!(decoded.error.is_none());
        assert_eq!(decoded.pay_preimage(), Some("abc123preimage".to_string()));
    }

    #[test]
    fn decode_get_balance_success() {
        let wallet_pk = wallet_pk();
        let response = json!({
            "result_type": "get_balance",
            "error": null,
            "result": { "balance": 150_000_u64 }
        });
        let frame = relay_event(23195, &wallet_pk, "evt-2", &response);
        let decoded = try_decode_relay_message(&frame, &wallet_pk, CLIENT_SECRET).unwrap();
        assert_eq!(decoded.balance_msats(), Some(150_000));
    }

    /// An `UNAUTHORIZED` error response must decode cleanly with `error` set
    /// and the typed accessors returning None (not the stale/absent result).
    #[test]
    fn decode_error_response_unauthorized() {
        let wallet_pk = wallet_pk();
        let response = json!({
            "result_type": "pay_invoice",
            "error": { "code": "UNAUTHORIZED", "message": "permission denied" },
            "result": null
        });
        let frame = relay_event(23195, &wallet_pk, "evt-3", &response);
        let decoded = try_decode_relay_message(&frame, &wallet_pk, CLIENT_SECRET).unwrap();
        let err = decoded.error.as_ref().expect("error must be present");
        assert_eq!(err.code, "UNAUTHORIZED");
        assert_eq!(err.message, "permission denied");
        assert_eq!(decoded.pay_preimage(), None, "error response yields no preimage");
    }

    #[test]
    fn decode_with_id_extracts_event_id() {
        let wallet_pk = wallet_pk();
        let response = json!({
            "result_type": "get_balance",
            "error": null,
            "result": { "balance": 42_u64 }
        });
        let frame = relay_event(23195, &wallet_pk, "the-event-id", &response);
        let (id, decoded) =
            try_decode_relay_message_with_id(&frame, &wallet_pk, CLIENT_SECRET).unwrap();
        assert_eq!(id, "the-event-id");
        assert_eq!(decoded.balance_msats(), Some(42));
    }

    /// A non-23195 kind (e.g. another wallet's request echo) must be ignored.
    #[test]
    fn decode_wrong_kind_returns_none() {
        let wallet_pk = wallet_pk();
        let response = json!({ "result_type": "get_balance", "error": null,
            "result": { "balance": 1_u64 } });
        let frame = relay_event(23196, &wallet_pk, "evt", &response);
        assert!(try_decode_relay_message(&frame, &wallet_pk, CLIENT_SECRET).is_none());
    }

    /// An event from a different pubkey must not be accepted — this is the
    /// authenticity check that stops a spoofed wallet response.
    #[test]
    fn decode_wrong_pubkey_returns_none() {
        let real_wallet = wallet_pk();
        let imposter = "0303030303030303030303030303030303030303030303030303030303030303";
        let imposter_pk = crypto::client_pubkey_hex(imposter).unwrap();
        let response = json!({ "result_type": "get_balance", "error": null,
            "result": { "balance": 1_u64 } });
        // Frame carries the imposter's pubkey; we ask to decode as `real_wallet`.
        let frame = relay_event(23195, &imposter_pk, "evt", &response);
        assert!(try_decode_relay_message(&frame, &real_wallet, CLIENT_SECRET).is_none());
    }

    /// Non-EVENT relay frames (OK, NOTICE, EOSE) must be ignored, not parsed.
    #[test]
    fn decode_non_event_message_returns_none() {
        let wallet_pk = wallet_pk();
        for frame in [
            r#"["OK","evt-id",true,""]"#,
            r#"["NOTICE","some message"]"#,
            r#"["EOSE","sub-1"]"#,
        ] {
            assert!(
                try_decode_relay_message(frame, &wallet_pk, CLIENT_SECRET).is_none(),
                "non-EVENT frame {frame:?} must decode to None"
            );
        }
    }

    /// Malformed / truncated JSON must return None gracefully — never panic (D6).
    #[test]
    fn decode_malformed_json_returns_none() {
        let wallet_pk = wallet_pk();
        for frame in ["", "not json", "[", r#"["EVENT"]"#, r#"{"kind":23195}"#] {
            assert!(
                try_decode_relay_message(frame, &wallet_pk, CLIENT_SECRET).is_none(),
                "malformed frame {frame:?} must decode to None"
            );
        }
    }

    /// If the response content cannot be decrypted with the stored client
    /// secret (wrong key), decoding must fail to None, never panic.
    #[test]
    fn decode_with_wrong_client_secret_returns_none() {
        let wallet_pk = wallet_pk();
        let response = json!({ "result_type": "get_balance", "error": null,
            "result": { "balance": 1_u64 } });
        let frame = relay_event(23195, &wallet_pk, "evt", &response);
        let wrong_secret =
            "0404040404040404040404040404040404040404040404040404040404040404";
        assert!(try_decode_relay_message(&frame, &wallet_pk, wrong_secret).is_none());
    }

    /// Content that decrypts cleanly but is not a valid `NwcResponse` shape
    /// must yield None, not a panic.
    #[test]
    fn decode_decrypts_but_bad_response_shape_returns_none() {
        let wallet_pk = wallet_pk();
        // Valid JSON, but missing the required `result_type` field.
        let response = json!({ "unexpected": "payload" });
        let frame = relay_event(23195, &wallet_pk, "evt", &response);
        assert!(try_decode_relay_message(&frame, &wallet_pk, CLIENT_SECRET).is_none());
    }

    // ── try_decode_response_for_request — `e` tag extraction ────────────────

    /// Build a kind:23195 frame WITH a NIP-47 §3.2 `e` tag pointing to
    /// `request_event_id`. The new `try_decode_response_for_request` decoder
    /// keys off this tag, not the wrapper's own id.
    fn relay_event_with_e_tag(
        kind: u64,
        pubkey: &str,
        wrapper_id: &str,
        request_event_id: &str,
        response_json: &serde_json::Value,
    ) -> String {
        let client_pk = crypto::client_pubkey_hex(CLIENT_SECRET).unwrap();
        let plaintext = serde_json::to_string(response_json).unwrap();
        let content = crypto::encrypt(WALLET_SECRET, &client_pk, &plaintext).unwrap();
        let frame = json!([
            "EVENT",
            "sub-1",
            {
                "id": wrapper_id,
                "kind": kind,
                "pubkey": pubkey,
                "content": content,
                "tags": [["e", request_event_id]],
            }
        ]);
        serde_json::to_string(&frame).unwrap()
    }

    /// The decoder returns the **request** id from the `e` tag, not the
    /// wrapper id. That distinction is what closes the dispatched-payment
    /// promise — the client never saw the wrapper id at send time.
    #[test]
    fn decode_response_for_request_returns_e_tag_value() {
        let wallet_pk = wallet_pk();
        let response = json!({
            "result_type": "pay_invoice",
            "error": null,
            "result": { "preimage": "feed" }
        });
        let frame = relay_event_with_e_tag(
            23195,
            &wallet_pk,
            "wrapper-id",
            "the-request-id",
            &response,
        );
        let (req_id, decoded) =
            try_decode_response_for_request(&frame, &wallet_pk, CLIENT_SECRET).unwrap();
        assert_eq!(
            req_id, "the-request-id",
            "the returned id must come from the `e` tag, not the wrapper id",
        );
        assert_eq!(decoded.result_type, "pay_invoice");
    }

    /// A kind:23195 frame missing its NIP-47 §3.2 `e` tag is malformed —
    /// without it the client cannot correlate the reply to any request, and
    /// the decoder must fail closed.
    #[test]
    fn decode_response_for_request_without_e_tag_returns_none() {
        let wallet_pk = wallet_pk();
        let response = json!({
            "result_type": "pay_invoice",
            "error": null,
            "result": { "preimage": "feed" }
        });
        // The existing `relay_event` helper deliberately omits `tags` — a
        // response with no `e` tag must NOT decode through the new function
        // even though `try_decode_relay_message_with_id` accepts it.
        let frame = relay_event(23195, &wallet_pk, "wrapper-id", &response);
        assert!(
            try_decode_response_for_request(&frame, &wallet_pk, CLIENT_SECRET).is_none(),
            "a kind:23195 missing its `e` tag is unmatchable — must decode to None",
        );
    }

    /// The decoder ignores positional fields beyond the `e` tag value (e.g.
    /// a relay-hint third element) — the spec allows them and the matcher
    /// must tolerate them.
    #[test]
    fn decode_response_for_request_tolerates_extra_tag_fields() {
        let wallet_pk = wallet_pk();
        let client_pk = crypto::client_pubkey_hex(CLIENT_SECRET).unwrap();
        let response = json!({
            "result_type": "pay_invoice",
            "error": null,
            "result": { "preimage": "feed" }
        });
        let plaintext = serde_json::to_string(&response).unwrap();
        let content = crypto::encrypt(WALLET_SECRET, &client_pk, &plaintext).unwrap();
        let frame = json!([
            "EVENT",
            "sub-1",
            {
                "id": "wrapper",
                "kind": 23195u64,
                "pubkey": &wallet_pk,
                "content": content,
                "tags": [
                    ["p", "ignored-pubkey"],
                    ["e", "the-request-id", "wss://relay.hint"],
                ],
            }
        ])
        .to_string();
        let (req_id, _) =
            try_decode_response_for_request(&frame, &wallet_pk, CLIENT_SECRET).unwrap();
        assert_eq!(
            req_id, "the-request-id",
            "the matcher must accept a 3+ element `e` tag and read element 1",
        );
    }

    /// An error response with an `e` tag still decodes through the new
    /// function — the response handler needs to see both the request id and
    /// the typed `error` object to record the dispatched action as `Failed`.
    #[test]
    fn decode_response_for_request_returns_error_payload() {
        let wallet_pk = wallet_pk();
        let response = json!({
            "result_type": "pay_invoice",
            "error": { "code": "PAYMENT_FAILED", "message": "no route" },
            "result": null
        });
        let frame = relay_event_with_e_tag(
            23195,
            &wallet_pk,
            "wrapper",
            "req-fail",
            &response,
        );
        let (req_id, decoded) =
            try_decode_response_for_request(&frame, &wallet_pk, CLIENT_SECRET).unwrap();
        assert_eq!(req_id, "req-fail");
        let err = decoded.error.expect("error must round-trip");
        assert_eq!(err.code, "PAYMENT_FAILED");
        assert_eq!(err.message, "no route");
    }
}
