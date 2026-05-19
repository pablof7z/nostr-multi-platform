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
