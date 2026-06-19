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
//!   sender + fallback amount source + zap-request id).
//!
//! ## Receipt integrity — what this decoder enforces
//!
//! NIP-57 has two integrity rules over the zap-receipt's `bolt11` invoice and
//! its embedded `description` (the kind:9734 zap request):
//!
//! - **MUST (enforced here):** the bolt11 invoice amount MUST equal the embedded
//!   zap request's `amount` tag, when both are present. A relay that rewrites
//!   the `description` to embed a different `amount` (or a forged sender) is
//!   detected by this mismatch. On a contradiction this decoder distrusts the
//!   *description-derived* fields — `amount_msats` falls back to the
//!   authoritative bolt11 HRP value and the description-derived `sender_pubkey`
//!   is dropped. See [`decode_borrowed`].
//! - **SHOULD (not enforced):** `SHA-256(description)` SHOULD equal the
//!   *description hash* embedded in the bolt11 data part (the bolt11 `h` tag /
//!   tagged-field type 23, 32 bytes — NOT the payment hash, which is
//!   `SHA-256(preimage)` and unrelated to the zap request). Enforcing it
//!   requires full BOLT-11 bech32 data-part decoding plus a SHA-256 dependency;
//!   it is left as a follow-up. The amount-equality MUST check above already
//!   closes the practical "forged embedded amount" path.
//!
//! The uppercase `P`-tag sender is set by the LN provider independently of the
//! `description`, so it is trusted regardless of any description contradiction.

use nmp_store::StoredEvent;
use nmp_core::substrate::KernelEvent;
use nmp_core::tags::first_tag_value;
use serde::{Deserialize, Serialize};

use crate::bolt11;
use crate::kinds::KIND_ZAP_RECEIPT;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZapReceiptRecord {
    pub event_id: String,
    /// Kind:9735 event author. For locally tracked zap requests this must
    /// match the LNURL-pay endpoint's advertised `nostrPubkey`.
    pub provider_pubkey: String,
    pub recipient_pubkey: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zapped_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zapped_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zap_request_id: Option<String>,
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

#[must_use]
pub fn try_from_event(event: &StoredEvent) -> Option<ZapReceiptRecord> {
    let raw = event.raw.as_ref();
    decode_borrowed(&raw.id, &raw.pubkey, raw.kind, raw.created_at, &raw.tags)
}

#[must_use]
pub fn try_from_kernel_event(event: &KernelEvent) -> Option<ZapReceiptRecord> {
    decode_borrowed(
        &event.id,
        &event.author,
        event.kind,
        event.created_at,
        &event.tags,
    )
}

fn decode_borrowed(
    id: &str,
    author: &str,
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
    let parsed_request: Option<serde_json::Value> =
        description.and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());

    // The two independent amount sources.
    let bolt11_amount = bolt11.as_deref().and_then(bolt11::amount_msats);
    let embedded_amount = amount_from_embedded_request(parsed_request.as_ref());

    // NIP-57 MUST: the bolt11 invoice amount equals the embedded zap request's
    // `amount` tag when both are present. A mismatch means the `description`
    // was rewritten (by a relay or a forging intermediary) and its embedded
    // fields — `pubkey` (sender) and `amount` — cannot be trusted. The bolt11
    // HRP is what the LN provider actually settled, so it stays authoritative;
    // the description-derived sender is dropped.
    let description_contradicted = matches!(
        (bolt11_amount, embedded_amount),
        (Some(b), Some(e)) if b != e
    );

    // Sender precedence: explicit uppercase `P` tag wins (set by the LN
    // provider, independent of the `description`); else the embedded request's
    // `pubkey` field — but only when the description is not contradicted; else
    // None.
    let embedded_sender = if description_contradicted {
        None
    } else {
        parsed_request
            .as_ref()
            .and_then(|v| v.get("pubkey"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    let sender_pubkey = upper_sender.or(embedded_sender);
    let zap_request_id = parsed_request
        .as_ref()
        .and_then(|v| v.get("id"))
        .and_then(serde_json::Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string);

    // Amount precedence: bolt11 HRP (authoritative — the LN provider settled
    // exactly that); else the embedded zap-request's `amount` tag (millisats
    // as a string). On a contradiction the bolt11 value is already chosen by
    // this `or_else` ordering, so the forged embedded amount never surfaces.
    let amount_msats = bolt11_amount.or(embedded_amount);

    Some(ZapReceiptRecord {
        event_id: id.to_string(),
        provider_pubkey: author.to_string(),
        recipient_pubkey,
        zapped_event_id,
        zapped_address,
        zap_request_id,
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
        // A non-array element (e.g. `null`, a string scalar) is a malformed tag
        // from a hostile relay. Skip it — do NOT propagate `None` out of the
        // whole function, which would silently suppress a later well-formed
        // `["amount","<msats>"]` entry and let a forged receipt bypass the
        // amount-mismatch guard.
        let Some(arr) = t.as_array() else { continue };
        let Some(key) = arr.first().and_then(|v| v.as_str()) else {
            continue;
        };
        if key == "amount" {
            let Some(s) = arr.get(1).and_then(|v| v.as_str()) else {
                continue;
            };
            return s.parse::<u64>().ok();
        }
    }
    None
}

#[cfg(test)]
#[path = "decode_tests.rs"]
mod tests;
