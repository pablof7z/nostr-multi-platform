//! `RawEvent` ↔ `nostr::Event` conversion.
//!
//! ADR-0012 §"Decision": JSON round-trip per call is the M3 choice — simple
//! and correct. A future optimization can cache the parsed `nostr::Event`
//! inside `VerifiedEvent` (which already parses during `try_from_raw`).

use nostr::util::JsonUtil as _;
use nostr::Event;

use crate::types::{RawEvent, StoredEvent};
use crate::StoreError;

/// Serialize `RawEvent` to NIP-01 canonical JSON, then parse it into a
/// `nostr::Event`. This is the one entry point through which NMP-side data
/// reaches `nmp_nostr_lmdb::Lmdb::save_event_with_txn`.
pub(super) fn raw_to_nostr(raw: &RawEvent) -> Result<Event, StoreError> {
    let json = serde_json::to_string(raw)
        .map_err(|e| StoreError::Encoding(format!("raw_to_nostr: serialize: {e}")))?;
    Event::from_json(&json)
        .map_err(|e| StoreError::Encoding(format!("raw_to_nostr: parse: {e}")))
}

/// Parse a `nostr::Event` JSON blob back into a `RawEvent`.
///
/// Used by the query path: the fork returns `EventBorrow<'a>` which we
/// serialize via its `as_json()`/`From` impl and re-parse as `RawEvent`.
pub(super) fn nostr_to_raw(ev: &Event) -> Result<RawEvent, StoreError> {
    let json = ev.try_as_json()
        .map_err(|e| StoreError::Encoding(format!("nostr_to_raw: serialize: {e}")))?;
    serde_json::from_str(&json)
        .map_err(|e| StoreError::Encoding(format!("nostr_to_raw: parse: {e}")))
}

/// Wrap a `RawEvent` (already converted) into a `StoredEvent`.
pub(super) fn stored_from_raw(raw: RawEvent, received_at_ms: u64) -> StoredEvent {
    StoredEvent {
        raw: std::sync::Arc::new(raw),
        received_at_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RawEvent;
    use nostr::Keys;

    fn signed_raw() -> RawEvent {
        let keys = Keys::generate();
        let ev = nostr::EventBuilder::text_note("conv round-trip")
            .sign_with_keys(&keys)
            .expect("sign");
        let json = ev.try_as_json().expect("json");
        serde_json::from_str(&json).expect("parse")
    }

    #[test]
    fn raw_to_nostr_roundtrip_signed_event() {
        let raw = signed_raw();
        let ev = raw_to_nostr(&raw).expect("raw_to_nostr");
        let raw2 = nostr_to_raw(&ev).expect("nostr_to_raw");
        assert_eq!(raw.id, raw2.id);
        assert_eq!(raw.pubkey, raw2.pubkey);
        assert_eq!(raw.sig, raw2.sig);
        assert_eq!(raw.content, raw2.content);
        assert_eq!(raw.created_at, raw2.created_at);
        assert_eq!(raw.kind, raw2.kind);
    }
}
