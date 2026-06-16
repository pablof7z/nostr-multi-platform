//! Minimal NIP-59 decrypt-only surface.
//!
//! This crate exists for small notification-service style targets that need to
//! unwrap one kind:1059 gift-wrap with a local `nsec` without linking the NMP
//! actor, store, relay, or app FFI crates. Protocol parsing and verification are
//! delegated to `nmp-nip59`; this crate only adapts JSON + local key input to the
//! existing pure unwrap path.

use nostr::key::SecretKey;
use nostr::nips::nip19::FromBech32;
use nostr::{Event, JsonUtil, Keys};
use std::string::{String, ToString};

/// Unwrap a NIP-59 gift-wrap JSON envelope using a local `nsec`.
///
/// Returns the inner rumor as JSON. Errors are strings so thin hosts can surface
/// or log them without depending on the workspace's internal error types. The
/// returned error for a malformed `local_nsec` intentionally does not include the
/// supplied secret.
pub fn unwrap_gift_wrap(envelope_json: &str, local_nsec: &str) -> Result<String, String> {
    let secret =
        SecretKey::from_bech32(local_nsec).map_err(|_| "invalid local_nsec".to_string())?;
    let secp = nostr::secp256k1::Secp256k1::new();
    let receiver = Keys::new_with_ctx(&secp, secret);
    let envelope =
        Event::from_json(envelope_json).map_err(|e| format!("invalid envelope_json: {e}"))?;
    let unwrapped = nmp_nip59::unwrap_gift_wrap(&receiver, &envelope)
        .map_err(|e| format!("unwrap failed: {e}"))?;
    Ok(unwrapped.rumor.as_json())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_nip59::gift_wrap_local;
    use nostr::nips::nip19::ToBech32;
    use nostr::{EventBuilder, Kind, Tag, Timestamp, UnsignedEvent};

    fn sample_rumor(sender: &Keys, receiver: &Keys, content: &str) -> UnsignedEvent {
        EventBuilder::new(Kind::from_u16(14), content)
            .tag(Tag::public_key(receiver.public_key()))
            .custom_created_at(Timestamp::from(1_700_000_000))
            .build(sender.public_key())
    }

    fn gift_wrap_json(sender: &Keys, receiver: &Keys, content: &str) -> String {
        let rumor = sample_rumor(sender, receiver, content);
        gift_wrap_local(
            sender,
            &receiver.public_key(),
            &rumor,
            Timestamp::from(1_700_000_000),
        )
        .expect("gift wrap")
        .as_json()
    }

    fn nsec(keys: &Keys) -> String {
        keys.secret_key().to_bech32().expect("nsec")
    }

    #[test]
    fn unwrap_returns_inner_rumor_json() {
        let sender = Keys::generate();
        let receiver = Keys::generate();
        let envelope_json = gift_wrap_json(&sender, &receiver, "notification body");

        let rumor_json = unwrap_gift_wrap(&envelope_json, &nsec(&receiver)).expect("unwrap");
        let rumor = UnsignedEvent::from_json(&rumor_json).expect("rumor json");

        assert_eq!(rumor.pubkey, sender.public_key());
        assert_eq!(rumor.kind, Kind::from_u16(14));
        assert_eq!(rumor.content, "notification body");
    }

    #[test]
    fn malformed_nsec_is_rejected_without_echoing_secret() {
        let err = unwrap_gift_wrap("{}", "nsec1not-a-valid-secret").expect_err("invalid nsec");

        assert_eq!(err, "invalid local_nsec");
        assert!(!err.contains("nsec1not-a-valid-secret"));
    }

    #[test]
    fn wrong_local_nsec_cannot_unwrap_envelope() {
        let sender = Keys::generate();
        let receiver = Keys::generate();
        let wrong_receiver = Keys::generate();
        let envelope_json = gift_wrap_json(&sender, &receiver, "not yours");

        let err = unwrap_gift_wrap(&envelope_json, &nsec(&wrong_receiver)).expect_err("wrong key");

        assert!(err.contains("unwrap failed:"));
    }

    #[test]
    fn non_gift_wrap_event_is_rejected() {
        let sender = Keys::generate();
        let receiver = Keys::generate();
        let event_json = EventBuilder::new(Kind::TextNote, "plain note")
            .sign_with_keys(&sender)
            .expect("signed event")
            .as_json();

        let err = unwrap_gift_wrap(&event_json, &nsec(&receiver)).expect_err("not gift wrap");

        assert!(err.contains("not a gift-wrap event"));
    }
}
