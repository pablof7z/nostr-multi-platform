//! AUTH and OK frame parsers — the wire-side ingress for NIP-42.
//!
//! Frames arrive as parsed `serde_json::Value` arrays (the kernel's
//! `handle_text` does the outer `Vec<Value>` split). These parsers extract
//! the protocol-relevant fields and reject malformed frames silently
//! (caller logs).

use serde_json::Value;

/// Parsed `["AUTH", <challenge>]` frame the relay sends to demand auth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthChallenge {
    pub challenge: String,
    /// Relay URL the challenge arrived on. The driver uses this verbatim
    /// in the `["relay", <url>]` tag of the kind:22242.
    pub relay_url: String,
}

/// Parsed `["OK", <event_id>, <accepted>, <reason>]` frame. NIP-42 specs
/// the same wire shape NIP-01 uses for publish OKs; the only way to know
/// it's an AUTH ack is to match the event_id against the kind:22242 the
/// driver dispatched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthOk {
    pub event_id: String,
    pub accepted: bool,
    pub reason: String,
}

/// Parse an `["AUTH", <challenge>]` frame. Returns `None` when the frame
/// has the wrong shape or an empty challenge (NIP-42 requires a non-empty
/// challenge string; an empty one cannot produce a valid signable event).
pub fn parse_auth_frame(frame: &[Value], relay_url: &str) -> Option<AuthChallenge> {
    if frame.first().and_then(Value::as_str) != Some("AUTH") {
        return None;
    }
    let challenge = frame.get(1).and_then(Value::as_str)?.to_string();
    if challenge.is_empty() {
        return None;
    }
    Some(AuthChallenge {
        challenge,
        relay_url: relay_url.to_string(),
    })
}

/// Parse an `["OK", <event_id>, <accepted>, <reason>]` frame into the
/// fields the AUTH matcher needs. Returns `None` only when the frame
/// is malformed (missing or wrong-typed fields); non-AUTH OKs still parse
/// and the caller decides whether to route to AUTH or to publish.
pub fn parse_ok_frame(frame: &[Value]) -> Option<AuthOk> {
    if frame.first().and_then(Value::as_str) != Some("OK") {
        return None;
    }
    let event_id = frame.get(1).and_then(Value::as_str)?.to_string();
    let accepted = frame.get(2).and_then(Value::as_bool).unwrap_or(false);
    let reason = frame
        .get(3)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if event_id.is_empty() {
        return None;
    }
    Some(AuthOk {
        event_id,
        accepted,
        reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn auth_parser_extracts_challenge() {
        let frame = vec![json!("AUTH"), json!("abc123")];
        let parsed = parse_auth_frame(&frame, "wss://relay.example").unwrap();
        assert_eq!(parsed.challenge, "abc123");
        assert_eq!(parsed.relay_url, "wss://relay.example");
    }

    #[test]
    fn auth_parser_rejects_wrong_kind_or_empty() {
        assert!(parse_auth_frame(&[json!("NOTAUTH"), json!("x")], "wss://x").is_none());
        assert!(parse_auth_frame(&[json!("AUTH"), json!("")], "wss://x").is_none());
        assert!(parse_auth_frame(&[json!("AUTH")], "wss://x").is_none());
        assert!(parse_auth_frame(&[json!("AUTH"), json!(42)], "wss://x").is_none());
    }

    #[test]
    fn ok_parser_extracts_event_id_and_status() {
        let frame = vec![json!("OK"), json!("a".repeat(64)), json!(true), json!("")];
        let parsed = parse_ok_frame(&frame).unwrap();
        assert_eq!(parsed.event_id.len(), 64);
        assert!(parsed.accepted);
        assert_eq!(parsed.reason, "");
    }

    #[test]
    fn ok_parser_includes_rejection_reason() {
        let frame = vec![
            json!("OK"),
            json!("b".repeat(64)),
            json!(false),
            json!("restricted: subscribers only"),
        ];
        let parsed = parse_ok_frame(&frame).unwrap();
        assert!(!parsed.accepted);
        assert_eq!(parsed.reason, "restricted: subscribers only");
    }

    #[test]
    fn ok_parser_tolerates_missing_reason() {
        let frame = vec![json!("OK"), json!("c".repeat(64)), json!(true)];
        let parsed = parse_ok_frame(&frame).unwrap();
        assert!(parsed.accepted);
        assert!(parsed.reason.is_empty());
    }

    #[test]
    fn ok_parser_rejects_malformed() {
        assert!(parse_ok_frame(&[json!("OK")]).is_none());
        assert!(parse_ok_frame(&[json!("OK"), json!("")]).is_none());
        assert!(parse_ok_frame(&[json!("NOK"), json!("a")]).is_none());
    }
}
