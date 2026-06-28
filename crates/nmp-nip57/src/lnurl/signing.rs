//! kind:9734 zap-request signing + flat NIP-01 re-serialization.
//!
//! Split out of `lnurl/mod.rs` (AGENTS.md 500-LOC ceiling). Both helpers
//! bridge the substrate's typed [`UnsignedEvent`] / [`SignedEvent`] shapes to
//! the `nostr` crate's flat NIP-01 wire form the LNURL callback expects in its
//! `nostr=<urlencoded>` parameter. Re-exported from the crate root via
//! `lnurl::sign_zap_request`.

use std::str::FromStr;

use nmp_signer_iface::{SignedEvent, UnsignedEvent};
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

/// Sign `unsigned` with `keys` and emit the flat NIP-01 JSON object the
/// LNURL callback expects in its `nostr=<urlencoded>` parameter.
///
/// Mirrors the wallet-runtime `sign_nwc_request` precedent — build a
/// `nostr::Event` via `EventBuilder`, then re-serialize to JSON. The reseat
/// step is the bridge between the substrate's typed `UnsignedEvent` shape
/// (kind / tags / content / `created_at`) and the nostr crate's signer API.
pub fn sign_zap_request(keys: &Keys, unsigned: &UnsignedEvent) -> Result<String, String> {
    let kind = Kind::from_u16(
        u16::try_from(unsigned.kind).map_err(|e| format!("zap kind out of range: {e}"))?,
    );
    let tags: Vec<Tag> = unsigned
        .tags
        .iter()
        .map(|t| {
            Tag::parse(
                t.iter()
                    .map(std::string::String::as_str)
                    .collect::<Vec<_>>(),
            )
            .map_err(|e| format!("tag parse: {e}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let event = EventBuilder::new(kind, unsigned.content.clone())
        .tags(tags)
        .custom_created_at(Timestamp::from(unsigned.created_at))
        .sign_with_keys(keys)
        .map_err(|e| format!("sign: {e}"))?;
    serde_json::to_string(&event).map_err(|e| format!("serialize signed zap request: {e}"))
}

/// V-78 — re-serialize a substrate [`SignedEvent`] into the flat NIP-01 JSON
/// object the LNURL callback expects in its `nostr=<urlencoded>` parameter.
///
/// The substrate [`SignedEvent`] is a nested `{ id, sig, unsigned: { … } }`
/// shape; the LN provider needs the flat `{ id, pubkey, created_at, kind,
/// tags, content, sig }` NIP-01 wire form. We reconstruct a `nostr::Event`
/// from the signed fields and serialize it through the SAME `serde` path
/// [`sign_zap_request`] uses — so a bunker-signed zap request is byte-for-byte
/// the wire shape a local-nsec zap produced, the moment the broker returns the
/// `id`/`sig`. No re-signing: the kind:9734 signature minted by the active
/// account (local OR bunker) is carried through verbatim.
pub fn signed_event_to_nostr_json(signed: &SignedEvent) -> Result<String, String> {
    let SignedEvent { id, sig, unsigned } = signed;

    let event_id = nostr::EventId::from_hex(id).map_err(|e| format!("zap event id: {e}"))?;
    let pubkey =
        nostr::PublicKey::from_hex(&unsigned.pubkey).map_err(|e| format!("zap pubkey: {e}"))?;
    let signature = nostr::secp256k1::schnorr::Signature::from_str(sig)
        .map_err(|e| format!("zap signature: {e}"))?;
    let kind = Kind::from_u16(
        u16::try_from(unsigned.kind).map_err(|e| format!("zap kind out of range: {e}"))?,
    );
    let tags: Vec<Tag> = unsigned
        .tags
        .iter()
        .map(|t| {
            Tag::parse(t.iter().map(String::as_str).collect::<Vec<_>>())
                .map_err(|e| format!("tag parse: {e}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let event = nostr::Event::new(
        event_id,
        pubkey,
        Timestamp::from(unsigned.created_at),
        kind,
        tags,
        unsigned.content.clone(),
        signature,
    );
    serde_json::to_string(&event).map_err(|e| format!("serialize signed zap request: {e}"))
}
