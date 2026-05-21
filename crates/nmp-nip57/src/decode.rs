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
//!
//! ## Security precondition (not yet enforced)
//!
//! NIP-57 Appendix F requires that SHA-256(description_tag_raw_bytes) equals
//! the payment hash embedded in the bolt11 data part (type 1 field, 32 bytes).
//! This decoder does NOT yet perform that check — doing so requires BOLT-11
//! bech32 data-part decoding and a SHA-256 dependency (`sha2` crate). Until
//! that check is added, a receipt with a mismatched description could surface a
//! forged `sender_pubkey` or `amount_msats`. **Do not use decoded fields for
//! authorization decisions until this check lands.**

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

    #[test]
    fn no_amount_source_yields_none_amount_not_panic() {
        // A receipt with neither a `bolt11` tag nor a `description` carries no
        // amount at all — the field must be `None`, never a panic or a guess.
        let tags = vec![vec!["p".into(), "recipient".into()]];
        let r = try_from_event(&make_stored(9735, tags)).unwrap();
        assert!(r.amount_msats.is_none());
        assert!(r.bolt11.is_none());
        assert!(r.sender_pubkey.is_none());
    }

    #[test]
    fn malformed_bolt11_without_embedded_amount_yields_none_amount() {
        // bolt11 is present but unparseable (no `ln*` prefix) and there is no
        // embedded request to fall back on → amount is `None`, bolt11 still
        // carried through verbatim for diagnostics.
        let tags = vec![
            vec!["p".into(), "recipient".into()],
            vec!["bolt11".into(), "not-a-real-invoice".into()],
        ];
        let r = try_from_event(&make_stored(9735, tags)).unwrap();
        assert!(r.amount_msats.is_none());
        assert_eq!(r.bolt11.as_deref(), Some("not-a-real-invoice"));
    }

    #[test]
    fn empty_bolt11_string_yields_none_amount() {
        let tags = vec![
            vec!["p".into(), "recipient".into()],
            vec!["bolt11".into(), String::new()],
        ];
        let r = try_from_event(&make_stored(9735, tags)).unwrap();
        assert!(r.amount_msats.is_none());
    }

    #[test]
    fn embedded_amount_non_numeric_falls_through_to_none() {
        // The embedded zap-request's `amount` tag holds a non-numeric string;
        // the `.parse::<u64>()` fails and the decoder yields `None` rather than
        // surfacing a bogus amount.
        let bad = r#"{"pubkey":"s","tags":[["amount","not-a-number"]]}"#;
        let tags = vec![
            vec!["p".into(), "recipient".into()],
            vec!["description".into(), bad.into()],
        ];
        let r = try_from_event(&make_stored(9735, tags)).unwrap();
        assert!(r.amount_msats.is_none());
        // The sender pubkey is still recovered from the embedded request.
        assert_eq!(r.sender_pubkey.as_deref(), Some("s"));
    }

    #[test]
    fn embedded_amount_negative_string_falls_through_to_none() {
        // A negative amount cannot parse as `u64` — must not panic, yields None.
        let bad = r#"{"pubkey":"s","tags":[["amount","-500"]]}"#;
        let tags = vec![
            vec!["p".into(), "recipient".into()],
            vec!["description".into(), bad.into()],
        ];
        let r = try_from_event(&make_stored(9735, tags)).unwrap();
        assert!(r.amount_msats.is_none());
    }

    #[test]
    fn addressable_target_a_tag_is_extracted() {
        // A zap aimed at a long-form / addressable event carries an `a`
        // coordinate instead of (or alongside) an `e` id.
        let tags = vec![
            vec!["p".into(), "recipient".into()],
            vec!["a".into(), "30023:authorpk:my-article".into()],
        ];
        let r = try_from_event(&make_stored(9735, tags)).unwrap();
        assert_eq!(r.zapped_address.as_deref(), Some("30023:authorpk:my-article"));
        assert!(r.zapped_event_id.is_none());
    }

    #[test]
    fn zap_to_profile_has_no_event_or_address_target() {
        // A direct profile zap names only the recipient `p` — no `e`, no `a`.
        // This is a valid receipt and must decode cleanly.
        let tags = vec![
            vec!["p".into(), "recipient".into()],
            vec!["bolt11".into(), "lnbc21n1pvj...".into()],
        ];
        let r = try_from_event(&make_stored(9735, tags)).unwrap();
        assert_eq!(r.recipient_pubkey, "recipient");
        assert!(r.zapped_event_id.is_none());
        assert!(r.zapped_address.is_none());
        assert_eq!(r.amount_msats, Some(2_100));
    }

    #[test]
    fn private_zap_with_opaque_encrypted_description_exposes_no_sender() {
        // NIP-57 private zaps replace the JSON request in `description` with an
        // opaque encrypted blob. It is not valid JSON, so neither a sender nor
        // an amount can be recovered — and there is no uppercase `P` tag.
        let tags = vec![
            vec!["p".into(), "recipient".into()],
            vec![
                "description".into(),
                "A1B2C3D4E5F6==encrypted-private-zap-payload==".into(),
            ],
            vec!["bolt11".into(), "lnbc10n1pvj...".into()],
        ];
        let r = try_from_event(&make_stored(9735, tags)).unwrap();
        // Sender stays hidden — the private-zap invariant.
        assert!(r.sender_pubkey.is_none());
        // The settled amount still comes from the authoritative bolt11 HRP.
        assert_eq!(r.amount_msats, Some(1_000));
    }

    #[test]
    fn first_e_tag_wins_when_receipt_lists_multiple() {
        // A malformed/relay-mangled receipt with two `e` tags: the decoder is
        // deterministic — it pins the first.
        let tags = vec![
            vec!["p".into(), "recipient".into()],
            vec!["e".into(), "FIRST_NOTE".into()],
            vec!["e".into(), "SECOND_NOTE".into()],
        ];
        let r = try_from_event(&make_stored(9735, tags)).unwrap();
        assert_eq!(r.zapped_event_id.as_deref(), Some("FIRST_NOTE"));
    }

    #[test]
    fn try_from_kernel_event_decodes_equivalently() {
        use nmp_core::substrate::KernelEvent;
        let kernel = KernelEvent {
            id: "k".repeat(64),
            author: "ln_node".into(),
            kind: 9735,
            created_at: 1_700_000_001,
            tags: vec![
                vec!["p".into(), "recipient".into()],
                vec!["e".into(), "NOTE".into()],
                vec!["bolt11".into(), "lnbc500u1pvj...".into()],
            ],
            content: String::new(),
        };
        let r = try_from_kernel_event(&kernel).unwrap();
        assert_eq!(r.event_id, "k".repeat(64));
        assert_eq!(r.recipient_pubkey, "recipient");
        assert_eq!(r.zapped_event_id.as_deref(), Some("NOTE"));
        assert_eq!(r.amount_msats, Some(50_000_000));
        // A non-receipt kernel event is rejected.
        let not_receipt = KernelEvent { kind: 1, ..kernel };
        assert!(try_from_kernel_event(&not_receipt).is_none());
    }
}
