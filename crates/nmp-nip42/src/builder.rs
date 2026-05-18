//! Kind:22242 builder — the unsigned event template handed to the signer.
//!
//! Per NIP-42:
//! - kind = 22242
//! - tags = [ ["relay", <relay_url>], ["challenge", <challenge>] ]
//! - content = "" (empty)
//! - created_at = within 10 minutes of relay-side "now"
//!
//! The signer fills in `id` / `pubkey` / `sig`.

use nmp_core::substrate::UnsignedEvent;

use super::frame::AuthChallenge;

/// Build the unsigned kind:22242 template for `challenge`. `pubkey` is the
/// account the signer will sign with; `created_at` is unix-seconds at
/// caller's clock (the signer accepts any value, but a wall-clock skew
/// beyond ±10 minutes will cause the relay to reject the AUTH event).
pub fn build_auth_event(
    challenge: &AuthChallenge,
    pubkey: String,
    created_at: u64,
) -> UnsignedEvent {
    UnsignedEvent {
        pubkey,
        kind: 22242,
        tags: vec![
            vec!["relay".to_string(), challenge.relay_url.clone()],
            vec!["challenge".to_string(), challenge.challenge.clone()],
        ],
        content: String::new(),
        created_at,
    }
}

/// Validate that a signed event the signer returned actually looks like the
/// kind:22242 we asked for. The signer's pubkey and signature are checked
/// elsewhere (the kernel's `verify_and_persist` path runs Schnorr verify);
/// this is only the structural shape check.
///
/// Catches buggy signers that mutate the template before signing (the
/// `applesauce` `SignerMismatchError` class).
pub fn validate_signed_for(
    signed: &nmp_core::substrate::SignedEvent,
    challenge: &AuthChallenge,
) -> Result<(), String> {
    if signed.unsigned.kind != 22242 {
        return Err(format!(
            "signer returned kind {}, expected 22242",
            signed.unsigned.kind
        ));
    }
    if signed.id.len() != 64 || !signed.id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("signer returned malformed event id".to_string());
    }
    if signed.sig.is_empty() {
        return Err("signer returned empty signature".to_string());
    }
    let echoed = signed.unsigned.tags.iter().any(|tag| {
        tag.first().map(String::as_str) == Some("challenge")
            && tag.get(1).map(String::as_str) == Some(challenge.challenge.as_str())
    });
    if !echoed {
        return Err("signer returned event missing challenge tag echo".to_string());
    }
    let relay_echoed = signed.unsigned.tags.iter().any(|tag| {
        tag.first().map(String::as_str) == Some("relay")
            && tag.get(1).map(String::as_str) == Some(challenge.relay_url.as_str())
    });
    if !relay_echoed {
        return Err("signer returned event missing relay tag echo".to_string());
    }
    Ok(())
}

/// Render the wire frame the kernel pushes to the relay:
/// `["AUTH", <event_json>]`. The event_json shape is the standard NIP-01
/// signed-event object (id, pubkey, created_at, kind, tags, content, sig).
pub fn wire_frame_for(signed: &nmp_core::substrate::SignedEvent) -> String {
    serde_json::json!([
        "AUTH",
        {
            "id": signed.id,
            "pubkey": signed.unsigned.pubkey,
            "created_at": signed.unsigned.created_at,
            "kind": signed.unsigned.kind,
            "tags": signed.unsigned.tags,
            "content": signed.unsigned.content,
            "sig": signed.sig,
        }
    ])
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::{SignedEvent, UnsignedEvent};

    fn fresh_challenge() -> AuthChallenge {
        AuthChallenge {
            challenge: "deadbeef".to_string(),
            relay_url: "wss://relay.example".to_string(),
        }
    }

    #[test]
    fn unsigned_template_has_required_tags() {
        let unsigned = build_auth_event(&fresh_challenge(), "f".repeat(64), 1_700_000_000);
        assert_eq!(unsigned.kind, 22242);
        assert_eq!(unsigned.created_at, 1_700_000_000);
        assert!(unsigned.content.is_empty());
        assert_eq!(unsigned.pubkey.len(), 64);
        assert_eq!(
            unsigned.tags,
            vec![
                vec!["relay".to_string(), "wss://relay.example".to_string()],
                vec!["challenge".to_string(), "deadbeef".to_string()],
            ]
        );
    }

    fn good_signed(challenge: &AuthChallenge) -> SignedEvent {
        SignedEvent {
            id: "a".repeat(64),
            sig: "c".repeat(128),
            unsigned: UnsignedEvent {
                pubkey: "b".repeat(64),
                kind: 22242,
                tags: vec![
                    vec!["relay".to_string(), challenge.relay_url.clone()],
                    vec!["challenge".to_string(), challenge.challenge.clone()],
                ],
                content: String::new(),
                created_at: 1,
            },
        }
    }

    #[test]
    fn validate_accepts_well_formed_event() {
        let ch = fresh_challenge();
        assert!(validate_signed_for(&good_signed(&ch), &ch).is_ok());
    }

    #[test]
    fn validate_rejects_wrong_kind() {
        let ch = fresh_challenge();
        let mut signed = good_signed(&ch);
        signed.unsigned.kind = 1;
        assert!(validate_signed_for(&signed, &ch).is_err());
    }

    #[test]
    fn validate_rejects_missing_challenge_echo() {
        let ch = fresh_challenge();
        let mut signed = good_signed(&ch);
        signed.unsigned.tags = vec![vec!["challenge".to_string(), "different".to_string()]];
        assert!(validate_signed_for(&signed, &ch).is_err());
    }

    #[test]
    fn validate_rejects_missing_relay_echo() {
        let ch = fresh_challenge();
        let mut signed = good_signed(&ch);
        signed.unsigned.tags = vec![vec!["challenge".to_string(), ch.challenge.clone()]];
        assert!(validate_signed_for(&signed, &ch).is_err());
    }

    #[test]
    fn validate_rejects_malformed_id_or_empty_sig() {
        let ch = fresh_challenge();
        let mut bad_id = good_signed(&ch);
        bad_id.id = "short".to_string();
        assert!(validate_signed_for(&bad_id, &ch).is_err());

        let mut empty_sig = good_signed(&ch);
        empty_sig.sig = String::new();
        assert!(validate_signed_for(&empty_sig, &ch).is_err());
    }

    #[test]
    fn wire_frame_starts_with_auth_and_carries_event_fields() {
        let ch = fresh_challenge();
        let wire = wire_frame_for(&good_signed(&ch));
        assert!(wire.starts_with("[\"AUTH\","));
        assert!(wire.contains("\"kind\":22242"));
        assert!(wire.contains("\"sig\":\""));
    }
}
