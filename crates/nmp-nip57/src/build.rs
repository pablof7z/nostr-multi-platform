//! Zap **request** builder (kind:9734). Zap **receipts** (kind:9735) are
//! LN-minted: the LN provider settles the invoice and publishes the receipt,
//! so this crate offers no receipt builder by design.
//!
//! The request shape per NIP-57:
//! - `relays` tag: where the recipient should look for the receipt. Optional
//!   at build time — when the caller passes no relays the actor injects the
//!   recipient's NIP-65 (kind:10002) write/both list before signing (V-07).
//! - `amount` tag: msats as a base-10 string (optional but conventional).
//! - `p` tag: recipient.
//! - `e` tag (optional): zapped event id.
//! - `a` tag (optional): zapped addressable coord.
//! - `content`: free-form comment (optional).

use nmp_core::substrate::UnsignedEvent;
use nmp_core::tags::{e_tag, p_tag};
use serde::{Deserialize, Serialize};

use crate::kinds::KIND_ZAP_REQUEST;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ZapRequestBuildError {
    /// Recipient pubkey was empty after trim — NIP-57 requires a single `p`.
    MissingRecipient,
}

impl core::fmt::Display for ZapRequestBuildError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MissingRecipient => write!(f, "NIP-57 zap request requires a recipient `p` tag"),
        }
    }
}

impl std::error::Error for ZapRequestBuildError {}

pub struct ZapRequest;

impl ZapRequest {
    /// Begin a zap-request targeting `recipient_pubkey`.
    pub fn to_pubkey(recipient_pubkey: impl Into<String>) -> ZapRequestBuilder {
        ZapRequestBuilder {
            recipient: recipient_pubkey.into(),
            amount_msats: None,
            relays: Vec::new(),
            zapped_event_id: None,
            zapped_address: None,
            comment: String::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ZapRequestBuilder {
    recipient: String,
    amount_msats: Option<u64>,
    relays: Vec<String>,
    zapped_event_id: Option<String>,
    zapped_address: Option<String>,
    comment: String,
}

impl ZapRequestBuilder {
    #[must_use]
    pub fn amount_msats(mut self, msats: u64) -> Self {
        self.amount_msats = Some(msats);
        self
    }

    /// Set the `relays` list. Replaces any previous list. Whitespace-only
    /// entries are filtered out. An empty (or fully whitespace-filtered)
    /// list is valid: `build()` will omit the `relays` tag and the actor
    /// will inject the recipient's NIP-65 write list before signing (V-07).
    #[must_use]
    pub fn relays(mut self, relays: Vec<String>) -> Self {
        self.relays = relays.into_iter().filter(|r| !r.trim().is_empty()).collect();
        self
    }

    #[must_use]
    pub fn zapped_event(mut self, id: impl Into<String>) -> Self {
        self.zapped_event_id = Some(id.into());
        self
    }

    /// Set the zapped addressable coordinate as a `<kind>:<pubkey>:<d>` string,
    /// or pre-formatted `a_tag` value.
    #[must_use]
    pub fn zapped_address(mut self, coord: impl Into<String>) -> Self {
        self.zapped_address = Some(coord.into());
        self
    }

    #[must_use]
    pub fn comment(mut self, c: impl Into<String>) -> Self {
        self.comment = c.into();
        self
    }

    /// # Errors
    ///
    /// Returns [`ZapRequestBuildError`] if recipient or target is missing.
    pub fn build(
        self,
        author: impl Into<String>,
        created_at: u64,
    ) -> Result<UnsignedEvent, ZapRequestBuildError> {
        if self.recipient.trim().is_empty() {
            return Err(ZapRequestBuildError::MissingRecipient);
        }

        let mut tags: Vec<Vec<String>> = Vec::with_capacity(5);
        // V-07: skip the `relays` tag entirely when the caller passed none —
        // the actor injects the recipient's NIP-65 (kind:10002) write/both
        // list before signing. We must NOT emit a bare `["relays"]` row: that
        // would be a malformed NIP-57 tag.
        if !self.relays.is_empty() {
            let mut relays_tag = Vec::with_capacity(1 + self.relays.len());
            relays_tag.push("relays".to_string());
            relays_tag.extend(self.relays);
            tags.push(relays_tag);
        }

        if let Some(amt) = self.amount_msats {
            tags.push(vec!["amount".into(), amt.to_string()]);
        }
        tags.push(p_tag(&self.recipient, None));
        if let Some(eid) = self.zapped_event_id {
            tags.push(e_tag(&eid, None, None));
        }
        if let Some(coord) = self.zapped_address {
            // The caller supplies a pre-formatted "<kind>:<pubkey>:<d>"
            // coordinate string; we just enforce the column-1 "a" key.
            tags.push(vec!["a".to_string(), coord]);
        }

        Ok(UnsignedEvent {
            pubkey: author.into(),
            kind: KIND_ZAP_REQUEST,
            tags,
            content: self.comment,
            created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTHOR: &str = "deadbeef";

    fn tag_keys(unsigned: &UnsignedEvent) -> Vec<&str> {
        unsigned.tags.iter().filter_map(|t| t.first()).map(String::as_str).collect()
    }

    #[test]
    fn minimal_request_emits_relays_and_p() {
        let unsigned = ZapRequest::to_pubkey("alice")
            .relays(vec!["wss://r.x".into(), "wss://r.y".into()])
            .build(AUTHOR, 0)
            .unwrap();
        assert_eq!(unsigned.kind, KIND_ZAP_REQUEST);
        let keys = tag_keys(&unsigned);
        assert_eq!(keys, vec!["relays", "p"]);
        // relays tag is a single tag with multiple URLs.
        assert_eq!(unsigned.tags[0], vec!["relays", "wss://r.x", "wss://r.y"]);
        assert_eq!(unsigned.tags[1][1], "alice");
    }

    #[test]
    fn amount_emitted_when_set() {
        let unsigned = ZapRequest::to_pubkey("alice")
            .amount_msats(12_345)
            .relays(vec!["wss://r".into()])
            .build(AUTHOR, 0)
            .unwrap();
        assert_eq!(tag_keys(&unsigned), vec!["relays", "amount", "p"]);
        assert_eq!(unsigned.tags[1][1], "12345");
    }

    #[test]
    fn zapped_event_appends_e_tag() {
        let unsigned = ZapRequest::to_pubkey("alice")
            .relays(vec!["wss://r".into()])
            .zapped_event("NOTE_ID")
            .build(AUTHOR, 0)
            .unwrap();
        let keys = tag_keys(&unsigned);
        assert_eq!(keys, vec!["relays", "p", "e"]);
        assert_eq!(unsigned.tags[2][1], "NOTE_ID");
    }

    #[test]
    fn zapped_address_appends_a_tag() {
        let unsigned = ZapRequest::to_pubkey("alice")
            .relays(vec!["wss://r".into()])
            .zapped_address("30023:alice:intro")
            .build(AUTHOR, 0)
            .unwrap();
        let keys = tag_keys(&unsigned);
        assert_eq!(keys, vec!["relays", "p", "a"]);
        assert_eq!(unsigned.tags[2][1], "30023:alice:intro");
    }

    /// V-07: building with no relays (caller omits the list OR every entry
    /// is whitespace-only and gets filtered out) MUST succeed and emit NO
    /// `relays` tag — the actor will inject the recipient's NIP-65 write
    /// list before signing. We assert no bare `["relays"]` row leaks through
    /// either: that would be a malformed NIP-57 tag.
    #[test]
    fn no_relays_builds_without_relays_tag() {
        let unsigned = ZapRequest::to_pubkey("alice")
            .build(AUTHOR, 0)
            .expect("build must succeed without relays — actor injects them");
        let keys = tag_keys(&unsigned);
        assert!(
            !keys.contains(&"relays"),
            "no relays were supplied; tag must be absent (actor injects): {keys:?}"
        );
        assert_eq!(keys, vec!["p"]);
    }

    /// Whitespace-only entries filter to an empty list and follow the
    /// same auto-inject path — no `relays` tag emitted.
    #[test]
    fn whitespace_only_relays_filtered_to_empty_emits_no_tag() {
        let unsigned = ZapRequest::to_pubkey("alice")
            .relays(vec!["   ".into(), "\t".into()])
            .build(AUTHOR, 0)
            .expect("whitespace-only relays filter to empty; build must still succeed");
        let keys = tag_keys(&unsigned);
        assert!(
            !keys.contains(&"relays"),
            "whitespace-only relays were filtered; tag must be absent: {keys:?}"
        );
    }

    #[test]
    fn missing_recipient_errors() {
        let err = ZapRequest::to_pubkey("  ")
            .relays(vec!["wss://r".into()])
            .build(AUTHOR, 0)
            .unwrap_err();
        assert_eq!(err, ZapRequestBuildError::MissingRecipient);
    }

    #[test]
    fn comment_lands_in_content() {
        let unsigned = ZapRequest::to_pubkey("alice")
            .relays(vec!["wss://r".into()])
            .comment("nice post")
            .build(AUTHOR, 0)
            .unwrap();
        assert_eq!(unsigned.content, "nice post");
    }

    #[test]
    fn builder_consumes_self_compile_check() {
        let _: UnsignedEvent = ZapRequest::to_pubkey("a")
            .relays(vec!["wss://r".into()])
            .build(AUTHOR, 0)
            .unwrap();
    }
}
