//! Pure kind:24242 Blossom authorization builder (BUD-01/BUD-02) — NO I/O.
//!
//! Two pure functions, unit-tested in isolation:
//!
//! 1. [`build_upload_auth`] — construct the unsigned kind:24242 event for an
//!    upload: tags `["t","upload"]`, `["x",<sha256-hex>]`,
//!    `["expiration",<created_at + AUTH_TTL_SECS>]`, content `"Upload blob"`.
//!    The kernel owns `created_at` (D7); the caller passes it in.
//! 2. [`authorization_header_value`] — produce the
//!    `Nostr <base64(json(signed_event))>` value for the `Authorization` HTTP
//!    header from a signed kind:24242 event's flat NIP-01 JSON.
//!
//! All crypto/signing happens elsewhere (rust-nostr, via the kernel's
//! `SignEventForAccount` port) — this module only shapes the event and encodes
//! the header. The `pubkey` field of the returned [`UnsignedEvent`] is left
//! empty: the signer fills it from the resolved account.

use base64::Engine as _;
use nmp_core::substrate::UnsignedEvent;

use crate::kinds::KIND_BLOSSOM_AUTH;

/// Authorization-event lifetime in seconds (ADR-0043 user decision: 5 minutes).
/// The `expiration` tag is `created_at + AUTH_TTL_SECS`; a Blossom server
/// rejects the header after this instant. Long enough for a multi-MB upload
/// over a slow link, short enough that a leaked header is quickly useless.
pub const AUTH_TTL_SECS: u64 = 300;

/// Build the unsigned kind:24242 upload-authorization event.
///
/// * `sha256_hex` — lowercase hex SHA-256 of the blob (the `x` tag; the server
///   binds the authorization to exactly this blob).
/// * `created_at` — kernel wall-clock seconds (D7 — the caller, never this
///   pure builder, owns the clock). `expiration` is `created_at + AUTH_TTL_SECS`.
///
/// The returned event has an empty `pubkey`; the signer fills it from the
/// resolved account when the kernel signs it through the `SignEventForAccount`
/// port.
#[must_use]
pub fn build_upload_auth(sha256_hex: &str, created_at: u64) -> UnsignedEvent {
    let expiration = created_at.saturating_add(AUTH_TTL_SECS);
    UnsignedEvent {
        pubkey: String::new(),
        kind: KIND_BLOSSOM_AUTH,
        tags: vec![
            vec!["t".to_string(), "upload".to_string()],
            vec!["x".to_string(), sha256_hex.to_string()],
            vec!["expiration".to_string(), expiration.to_string()],
        ],
        content: "Upload blob".to_string(),
        created_at,
    }
}

/// Build the `Authorization` header VALUE for a signed kind:24242 event.
///
/// BUD-01 § "Authorization events": the header is `Nostr <base64>` where
/// `<base64>` is the standard base64 of the event's JSON. `signed_event_json`
/// is the flat NIP-01 JSON of the signed kind:24242 (as produced by the
/// kernel's signer). Returns e.g. `Nostr eyJpZCI6...`.
#[must_use]
pub fn authorization_header_value(signed_event_json: &str) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(signed_event_json.as_bytes());
    format!("Nostr {b64}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::nips::nip19::FromBech32;
    use nostr::{EventBuilder, Keys, Kind, SecretKey, Tag, Timestamp};

    const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
    const CREATED_AT: u64 = 1_700_000_000;

    /// Deterministic 64-hex SHA placeholder for tag-shape assertions.
    fn sha() -> String {
        "ab".repeat(32)
    }

    #[test]
    fn build_upload_auth_has_expected_tag_shape() {
        let ev = build_upload_auth(&sha(), CREATED_AT);
        assert_eq!(ev.kind, 24242);
        assert_eq!(ev.content, "Upload blob");
        assert_eq!(ev.created_at, CREATED_AT);
        assert!(ev.pubkey.is_empty(), "signer fills pubkey, not the builder");
        assert!(ev
            .tags
            .contains(&vec!["t".to_string(), "upload".to_string()]));
        assert!(ev.tags.contains(&vec!["x".to_string(), sha()]));
    }

    #[test]
    fn build_upload_auth_expiration_is_created_at_plus_300() {
        let ev = build_upload_auth(&sha(), CREATED_AT);
        let exp = ev
            .tags
            .iter()
            .find(|t| t.first().map(String::as_str) == Some("expiration"))
            .and_then(|t| t.get(1))
            .expect("expiration tag present");
        assert_eq!(
            exp,
            &(CREATED_AT + 300).to_string(),
            "expiration must be created_at + 300 (5 min TTL)"
        );
    }

    #[test]
    fn authorization_header_round_trips_to_signed_event_json() {
        // Sign the built event with a fixed key + fixed created_at, encode the
        // header, then base64-decode it back and assert it is the signed JSON.
        let keys = Keys::new(SecretKey::from_bech32(TEST_NSEC).unwrap());
        let unsigned = build_upload_auth(&sha(), CREATED_AT);
        let tags: Vec<Tag> = unsigned
            .tags
            .iter()
            .map(|t| Tag::parse(t).unwrap())
            .collect();
        let signed = EventBuilder::new(Kind::from_u16(24242), unsigned.content.clone())
            .tags(tags)
            .custom_created_at(Timestamp::from(CREATED_AT))
            .sign_with_keys(&keys)
            .unwrap();
        let signed_json = serde_json::to_string(&signed).unwrap();

        let header = authorization_header_value(&signed_json);
        assert!(header.starts_with("Nostr "), "scheme prefix: {header}");
        let b64 = header.strip_prefix("Nostr ").unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .expect("header payload must be valid base64");
        let decoded_str = String::from_utf8(decoded).expect("utf8");
        assert_eq!(
            decoded_str, signed_json,
            "base64 must decode back to the exact signed event JSON"
        );
        // And the decoded JSON is a valid signed event.
        let ev: nostr::Event = serde_json::from_str(&decoded_str).unwrap();
        assert_eq!(ev.kind.as_u16(), 24242);
        assert!(ev.verify().is_ok(), "decoded event signature verifies");
    }
}
