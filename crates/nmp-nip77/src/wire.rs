//! NIP-77 wire framing.
//!
//! Encodes / decodes the three frame types defined by NIP-77 as JSON arrays
//! ready to be shipped over a Nostr WebSocket relay.  The reconciler in
//! [`crate::reconciler`] is transport-agnostic — this module is the only
//! place that knows about WebSocket text frames and JSON tuples.
//!
//! ## Client → Relay
//!
//! ```json
//! ["NEG-OPEN", <subid>, <filter>, <initial-msg-hex>]
//! ["NEG-MSG",  <subid>, <msg-hex>]
//! ["NEG-CLOSE", <subid>]
//! ```
//!
//! ## Relay → Client
//!
//! ```json
//! ["NEG-MSG", <subid>, <msg-hex>]
//! ["NEG-ERR", <subid>, <reason>]
//! ```
//!
//! Hex encoding is lowercase per Nostr convention.

use serde_json::{json, Value};
use std::fmt;

/// Frames a client sends to a relay.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClientFrame {
    /// Begin a reconciliation on `sub_id` with the given filter and the
    /// client's initial negentropy bytes.
    Open {
        sub_id: String,
        filter: Value,
        initial_msg: Vec<u8>,
    },
    /// Continue an in-flight reconciliation.
    Msg { sub_id: String, msg: Vec<u8> },
    /// Tear down a reconciliation.
    Close { sub_id: String },
}

/// Frames a relay sends back to a client.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RelayFrame {
    Msg { sub_id: String, msg: Vec<u8> },
    Err { sub_id: String, reason: String },
}

#[derive(Debug, Eq, PartialEq)]
pub enum WireError {
    NotAnArray,
    UnknownVerb(String),
    MissingField(&'static str),
    InvalidType(&'static str),
    InvalidHex,
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnArray => f.write_str("relay frame must be a JSON array"),
            Self::UnknownVerb(v) => write!(f, "unknown verb: {v}"),
            Self::MissingField(name) => write!(f, "missing field: {name}"),
            Self::InvalidType(name) => write!(f, "invalid type for field: {name}"),
            Self::InvalidHex => f.write_str("invalid hex payload"),
        }
    }
}

impl std::error::Error for WireError {}

impl ClientFrame {
    /// Render this frame as a JSON string suitable for a WebSocket text
    /// payload.
    pub fn to_text(&self) -> String {
        let value: Value = match self {
            ClientFrame::Open {
                sub_id,
                filter,
                initial_msg,
            } => json!(["NEG-OPEN", sub_id, filter, hex_encode(initial_msg)]),
            ClientFrame::Msg { sub_id, msg } => json!(["NEG-MSG", sub_id, hex_encode(msg)]),
            ClientFrame::Close { sub_id } => json!(["NEG-CLOSE", sub_id]),
        };
        value.to_string()
    }
}

impl RelayFrame {
    /// Parse a relay frame from its JSON text representation.
    pub fn parse(text: &str) -> Result<Self, WireError> {
        let value: Value = serde_json::from_str(text).map_err(|_| WireError::NotAnArray)?;
        Self::from_value(&value)
    }

    /// Same as [`Self::parse`] but operates directly on a parsed `Value`.
    pub fn from_value(value: &Value) -> Result<Self, WireError> {
        let arr = value.as_array().ok_or(WireError::NotAnArray)?;
        let verb = arr
            .first()
            .and_then(|v| v.as_str())
            .ok_or(WireError::MissingField("verb"))?;
        match verb {
            "NEG-MSG" => {
                let sub_id = arr
                    .get(1)
                    .and_then(|v| v.as_str())
                    .ok_or(WireError::MissingField("sub_id"))?
                    .to_string();
                let msg_hex = arr
                    .get(2)
                    .and_then(|v| v.as_str())
                    .ok_or(WireError::MissingField("msg"))?;
                let msg = hex_decode(msg_hex)?;
                Ok(RelayFrame::Msg { sub_id, msg })
            }
            "NEG-ERR" => {
                let sub_id = arr
                    .get(1)
                    .and_then(|v| v.as_str())
                    .ok_or(WireError::MissingField("sub_id"))?
                    .to_string();
                let reason = arr
                    .get(2)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                Ok(RelayFrame::Err { sub_id, reason })
            }
            other => Err(WireError::UnknownVerb(other.to_string())),
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    static HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0F) as usize] as char);
    }
    out
}

fn hex_decode(s: &str) -> Result<Vec<u8>, WireError> {
    if !s.len().is_multiple_of(2) {
        return Err(WireError::InvalidHex);
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    for chunk in bytes.chunks(2) {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_nibble(b: u8) -> Result<u8, WireError> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(WireError::InvalidHex),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_roundtrip() {
        let frame = ClientFrame::Open {
            sub_id: "abc".into(),
            filter: json!({"kinds":[0],"authors":["aa"]}),
            initial_msg: vec![0x60, 0x01, 0x02],
        };
        let text = frame.to_text();
        assert!(text.starts_with("[\"NEG-OPEN\",\"abc\""));
        assert!(text.contains("\"600102\""));
    }

    #[test]
    fn msg_relay_parse() {
        let text = "[\"NEG-MSG\",\"sub1\",\"60aabb\"]";
        let parsed = RelayFrame::parse(text).unwrap();
        assert_eq!(
            parsed,
            RelayFrame::Msg {
                sub_id: "sub1".into(),
                msg: vec![0x60, 0xaa, 0xbb]
            }
        );
    }

    #[test]
    fn err_relay_parse() {
        let text = "[\"NEG-ERR\",\"sub1\",\"unsupported\"]";
        let parsed = RelayFrame::parse(text).unwrap();
        assert_eq!(
            parsed,
            RelayFrame::Err {
                sub_id: "sub1".into(),
                reason: "unsupported".into(),
            }
        );
    }

    #[test]
    fn rejects_unknown_verb() {
        let text = "[\"EOSE\",\"sub1\"]";
        let err = RelayFrame::parse(text).unwrap_err();
        assert!(matches!(err, WireError::UnknownVerb(ref v) if v == "EOSE"));
    }

    #[test]
    fn rejects_malformed_hex() {
        let text = "[\"NEG-MSG\",\"s\",\"xy\"]";
        let err = RelayFrame::parse(text).unwrap_err();
        assert_eq!(err, WireError::InvalidHex);
    }

    #[test]
    fn close_frame_serializes() {
        let frame = ClientFrame::Close {
            sub_id: "abc".into(),
        };
        assert_eq!(frame.to_text(), "[\"NEG-CLOSE\",\"abc\"]");
    }

    #[test]
    fn msg_client_frame_serializes() {
        // `ClientFrame::Msg` is the in-flight continuation frame; only Open
        // and Close serialization were previously covered.
        let frame = ClientFrame::Msg {
            sub_id: "s9".into(),
            msg: vec![0x60, 0xff, 0x00],
        };
        assert_eq!(frame.to_text(), "[\"NEG-MSG\",\"s9\",\"60ff00\"]");
    }

    #[test]
    fn empty_hex_body_decodes_to_empty_bytes() {
        // A NEG-MSG with an empty hex string is a valid zero-length payload,
        // not a malformed frame.
        let parsed = RelayFrame::parse("[\"NEG-MSG\",\"s\",\"\"]").unwrap();
        assert_eq!(
            parsed,
            RelayFrame::Msg {
                sub_id: "s".into(),
                msg: Vec::new()
            }
        );
    }

    #[test]
    fn uppercase_hex_body_is_accepted() {
        // The decoder accepts A-F as well as a-f; relays are not required to
        // emit lowercase even though NMP does.
        let parsed = RelayFrame::parse("[\"NEG-MSG\",\"s\",\"60AABB\"]").unwrap();
        assert_eq!(
            parsed,
            RelayFrame::Msg {
                sub_id: "s".into(),
                msg: vec![0x60, 0xaa, 0xbb]
            }
        );
    }

    #[test]
    fn neg_msg_missing_body_field_errors() {
        // NEG-MSG with no third element is structurally incomplete.
        let err = RelayFrame::parse("[\"NEG-MSG\",\"s\"]").unwrap_err();
        assert_eq!(err, WireError::MissingField("msg"));
    }

    #[test]
    fn odd_length_hex_body_rejected() {
        // Hex must be byte-aligned; an odd nibble count is invalid.
        let err = RelayFrame::parse("[\"NEG-MSG\",\"s\",\"abc\"]").unwrap_err();
        assert_eq!(err, WireError::InvalidHex);
    }

    #[test]
    fn non_array_frame_rejected() {
        let err = RelayFrame::parse("{\"verb\":\"NEG-MSG\"}").unwrap_err();
        assert_eq!(err, WireError::NotAnArray);
    }

    #[test]
    fn open_then_parse_back_msg_roundtrips_payload() {
        // `ClientFrame` has no public parser, but the NEG-MSG verb is shared:
        // a client-emitted NEG-MSG must parse back via `RelayFrame::parse`
        // with byte-identical payload — proving hex encode/decode are inverses.
        let original: Vec<u8> = (0u8..=255).collect();
        let text = ClientFrame::Msg {
            sub_id: "rt".into(),
            msg: original.clone(),
        }
        .to_text();
        match RelayFrame::parse(&text).unwrap() {
            RelayFrame::Msg { sub_id, msg } => {
                assert_eq!(sub_id, "rt");
                assert_eq!(msg, original);
            }
            other => panic!("expected Msg, got {other:?}"),
        }
    }
}
