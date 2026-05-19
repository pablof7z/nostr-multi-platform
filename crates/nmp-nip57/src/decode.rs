//! Decoder — `ZapReceiptRecord` from a kind:9735 zap receipt.
//!
//! Per NIP-57 a receipt carries:
//! - `p` (lowercase): the recipient pubkey (zap target).
//! - `e` (lowercase, optional): the zapped event id.
//! - `a` (lowercase, optional): the zapped addressable coordinate.
//! - `P` (uppercase, optional): the sender pubkey hint. Often absent; the
//!   embedded zap-request JSON in the `description` tag carries the
//!   authoritative `pubkey` field.
//! - `bolt11` (LN invoice — amount in the HRP is the authoritative number).
//! - `preimage` (optional).
//! - `description` (the embedded kind:9734 zap request as JSON, used as
//!   sender + fallback amount source).

use nmp_core::store::StoredEvent;
use nmp_core::substrate::KernelEvent;
use nmp_core::tags::first_tag_value;
use serde::{Deserialize, Serialize};

use crate::bolt11;
use crate::kinds::KIND_ZAP_RECEIPT;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZapReceiptRecord {
    pub event_id: String,
    pub recipient_pubkey: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zapped_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zapped_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_pubkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount_msats: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bolt11: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preimage: Option<String>,
    pub created_at: u64,
}

pub fn try_from_event(event: &StoredEvent) -> Option<ZapReceiptRecord> {
    let raw = event.raw.as_ref();
    decode_borrowed(&raw.id, raw.kind, raw.created_at, &raw.tags)
}

pub fn try_from_kernel_event(event: &KernelEvent) -> Option<ZapReceiptRecord> {
    decode_borrowed(&event.id, event.kind, event.created_at, &event.tags)
}

fn decode_borrowed(
    id: &str,
    kind: u32,
    created_at: u64,
    tags: &[Vec<String>],
) -> Option<ZapReceiptRecord> {
    if kind != KIND_ZAP_RECEIPT {
        return None;
    }
    let recipient_pubkey = first_tag_value(tags, "p")?.to_string();

    let zapped_event_id = first_tag_value(tags, "e").map(str::to_string);
    let zapped_address = first_tag_value(tags, "a").map(str::to_string);
    let upper_sender = first_tag_value(tags, "P").map(str::to_string);
    let bolt11 = first_tag_value(tags, "bolt11").map(str::to_string);
    let preimage = first_tag_value(tags, "preimage").map(str::to_string);

    let description = first_tag_value(tags, "description");
    let parsed_request: Option<serde_json::Value> = description
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());

    // Sender precedence: explicit uppercase `P` tag wins; else the embedded
    // request's `pubkey` field; else None.
    let sender_pubkey = upper_sender.or_else(|| {
        parsed_request
            .as_ref()
            .and_then(|v| v.get("pubkey"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    });

    // Amount precedence: bolt11 HRP (authoritative — the LN provider settled
    // exactly that); else the embedded zap-request's `amount` tag (millisats
    // as a string).
    let amount_msats = bolt11
        .as_deref()
        .and_then(bolt11::amount_msats)
        .or_else(|| amount_from_embedded_request(parsed_request.as_ref()));

    Some(ZapReceiptRecord {
        event_id: id.to_string(),
        recipient_pubkey,
        zapped_event_id,
        zapped_address,
        sender_pubkey,
        amount_msats,
        bolt11,
        preimage,
        created_at,
    })
}

fn amount_from_embedded_request(req: Option<&serde_json::Value>) -> Option<u64> {
    let tags = req?.get("tags")?.as_array()?;
    for t in tags {
        let arr = t.as_array()?;
        let key = arr.first()?.as_str()?;
        if key == "amount" {
            let s = arr.get(1)?.as_str()?;
            return s.parse::<u64>().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::store::{RawEvent, StoredEvent};
    use std::sync::Arc;

    fn make_stored(kind: u32, tags: Vec<Vec<String>>) -> StoredEvent {
        StoredEvent {
            raw: Arc::new(RawEvent {
                id: "a".repeat(64),
                pubkey: "b".repeat(64),
                created_at: 1_700_000_000,
                kind,
                tags,
                content: String::new(),
                sig: "c".repeat(128),
            }),
            received_at_ms: 0,
        }
    }

    fn embedded_request(pubkey: &str, amount_msats: u64) -> String {
        format!(
            r#"{{"pubkey":"{pk}","tags":[["amount","{amt}"]]}}"#,
            pk = pubkey,
            amt = amount_msats
        )
    }

    #[test]
    fn rejects_non_9735() {
        assert!(try_from_event(&make_stored(9734, vec![])).is_none());
        assert!(try_from_event(&make_stored(1, vec![])).is_none());
    }

    #[test]
    fn rejects_when_no_recipient() {
        assert!(try_from_event(&make_stored(9735, vec![])).is_none());
    }

    #[test]
    fn extracts_recipient_and_optional_event_target() {
        let tags = vec![
            vec!["p".into(), "alice".into()],
            vec!["e".into(), "ZAPPED_NOTE".into()],
        ];
        let r = try_from_event(&make_stored(9735, tags)).unwrap();
        assert_eq!(r.recipient_pubkey, "alice");
        assert_eq!(r.zapped_event_id.as_deref(), Some("ZAPPED_NOTE"));
        assert!(r.zapped_address.is_none());
        assert!(r.sender_pubkey.is_none());
        assert!(r.amount_msats.is_none());
    }

    #[test]
    fn uppercase_p_tag_wins_over_embedded_request() {
        let tags = vec![
            vec!["p".into(), "recipient".into()],
            vec!["P".into(), "explicit_sender".into()],
            vec!["description".into(), embedded_request("embedded_sender", 0)],
        ];
        let r = try_from_event(&make_stored(9735, tags)).unwrap();
        assert_eq!(r.sender_pubkey.as_deref(), Some("explicit_sender"));
    }

    #[test]
    fn embedded_request_pubkey_fills_sender_when_no_uppercase_p() {
        let tags = vec![
            vec!["p".into(), "recipient".into()],
            vec!["description".into(), embedded_request("embedded_sender", 1000)],
        ];
        let r = try_from_event(&make_stored(9735, tags)).unwrap();
        assert_eq!(r.sender_pubkey.as_deref(), Some("embedded_sender"));
    }

    #[test]
    fn bolt11_amount_wins_over_embedded_amount_tag() {
        let tags = vec![
            vec!["p".into(), "recipient".into()],
            vec!["bolt11".into(), "lnbc500u1pvj...".into()], // 50_000_000 msat
            vec!["description".into(), embedded_request("s", 999)],
        ];
        let r = try_from_event(&make_stored(9735, tags)).unwrap();
        assert_eq!(r.amount_msats, Some(50_000_000));
    }

    #[test]
    fn embedded_amount_used_when_bolt11_unparseable() {
        let tags = vec![
            vec!["p".into(), "recipient".into()],
            vec!["bolt11".into(), "lnbc1pvj...".into()], // no amount HRP → None
            vec!["description".into(), embedded_request("s", 1234)],
        ];
        let r = try_from_event(&make_stored(9735, tags)).unwrap();
        assert_eq!(r.amount_msats, Some(1234));
    }

    #[test]
    fn carries_preimage_and_bolt11_through() {
        let tags = vec![
            vec!["p".into(), "r".into()],
            vec!["bolt11".into(), "lnbc1m1pvj...".into()],
            vec!["preimage".into(), "abcd".into()],
        ];
        let r = try_from_event(&make_stored(9735, tags)).unwrap();
        assert_eq!(r.bolt11.as_deref(), Some("lnbc1m1pvj..."));
        assert_eq!(r.preimage.as_deref(), Some("abcd"));
        assert_eq!(r.amount_msats, Some(100_000_000));
    }

    #[test]
    fn malformed_description_does_not_panic() {
        let tags = vec![
            vec!["p".into(), "r".into()],
            vec!["description".into(), "{not json}".into()],
        ];
        let r = try_from_event(&make_stored(9735, tags)).unwrap();
        assert!(r.sender_pubkey.is_none());
        assert!(r.amount_msats.is_none());
    }
}
