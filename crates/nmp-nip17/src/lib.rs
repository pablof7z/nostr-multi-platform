//! `nmp-nip17` — NIP-17 private direct messages.
//!
//! # Overview
//!
//! NIP-17 DMs are *gift-wrapped* private messages. The send pipeline has three
//! layers:
//!
//! 1. **Rumor** — an *unsigned* kind:14 "chat message" event. It is NEVER
//!    signed and NEVER published directly. This crate builds the rumor.
//! 2. **Seal + gift-wrap** — the rumor is sealed (kind:13, NIP-44 from the
//!    sender) then gift-wrapped (kind:1059, NIP-44 from an ephemeral key) into
//!    an opaque envelope. NIP-59 (`nmp-nip59`) owns this step.
//! 3. **Routing** — one kind:1059 envelope per recipient plus one self-copy
//!    (the sender gift-wraps to their own pubkey so sent messages are
//!    readable), each published to the recipient's kind:10050 DM relay.
//!
//! # Scope of this crate (Phase 1)
//!
//! This crate is a **pure rumor builder**. It carries no key material, performs
//! no crypto, and emits no events. [`build_dm_rumor`] turns a [`DmInput`] into
//! an [`UnsignedEvent`] (kind:14). The gift-wrap and publish happen on the
//! actor thread (D7 — the kernel owns key access and the wall clock), driven by
//! `ActorCommand::SendGiftWrappedDm`.
//!
//! The actor's `SendGiftWrappedDm` arm is a **local-keys-only MVP**: a remote
//! (NIP-46 / bunker) signer cannot gift-wrap because `nmp_nip59::gift_wrap`
//! needs raw `nostr::Keys` for the NIP-44 seal. Bunker support is gated on
//! ADR-0026 (a NIP-44 encrypt/decrypt seam on `RemoteSignerHandle`). This
//! rumor-builder crate is signer-agnostic and unaffected by that gap.
//!
//! # D7: `created_at` sentinel
//!
//! The rumor is built with `created_at: 0`. The kernel owns the wall clock;
//! the actor re-stamps the timestamp from `kernel.now_secs()` before wrapping,
//! exactly as the other unsigned-event executors do. A `0` here is the
//! "kernel, please stamp me" sentinel, not epoch time.
//!
//! # Spec
//!
//! <https://github.com/nostr-protocol/nips/blob/master/17.md>

use nmp_core::substrate::UnsignedEvent;

/// NIP-17 kind: a "chat message" rumor.
const KIND_CHAT_MESSAGE: u32 = 14;

/// Caller intent for a single outgoing NIP-17 direct message.
///
/// This is the host-facing input shape. The recipient's gift-wrap envelope and
/// the sender self-copy are derived downstream on the actor thread; the host
/// only describes *what* to send, never *how* it is wrapped.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DmInput {
    /// Recipient's Nostr public key, lowercase hex (64 chars).
    pub recipient_pubkey: String,
    /// The plaintext message body. Becomes the kind:14 `content`.
    pub content: String,
    /// Optional event id (hex) this message replies to. When set, the rumor
    /// carries a NIP-10 style `e` tag with the `"reply"` marker.
    pub reply_to: Option<String>,
}

/// Build a NIP-17 kind:14 chat-message **rumor** (an unsigned event).
///
/// The returned [`UnsignedEvent`] is the inner rumor; it is never signed or
/// published as-is. The actor gift-wraps it (NIP-59) into one kind:1059
/// envelope per recipient plus a self-copy.
///
/// Tags:
/// - always a `["p", recipient_pubkey]` tag so the recipient is addressable.
/// - when `input.reply_to` is `Some`, an `["e", reply_to, "", "reply"]` tag
///   (NIP-10 reply marker; empty relay-hint slot).
///
/// `created_at` is set to `0` — the D7 sentinel. The actor re-stamps it from
/// the kernel clock before wrapping; this crate never reads the system clock.
pub fn build_dm_rumor(input: &DmInput, sender_pubkey: &str) -> UnsignedEvent {
    let mut tags: Vec<Vec<String>> = vec![vec![
        "p".to_string(),
        input.recipient_pubkey.clone(),
    ]];

    if let Some(reply_to) = &input.reply_to {
        // NIP-10 reply marker: ["e", <event-id>, <relay-hint>, "reply"].
        // The relay hint is left empty — Phase 1 has no relay-hint resolver.
        tags.push(vec![
            "e".to_string(),
            reply_to.clone(),
            String::new(),
            "reply".to_string(),
        ]);
    }

    UnsignedEvent {
        pubkey: sender_pubkey.to_string(),
        kind: KIND_CHAT_MESSAGE,
        tags,
        content: input.content.clone(),
        // D7 sentinel — the actor re-stamps from `kernel.now_secs()`.
        created_at: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SENDER: &str = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
    const RECIPIENT: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";

    #[test]
    fn build_dm_rumor_produces_kind_14() {
        let input = DmInput {
            recipient_pubkey: RECIPIENT.to_string(),
            content: "hello there".to_string(),
            reply_to: None,
        };
        let rumor = build_dm_rumor(&input, SENDER);
        assert_eq!(rumor.kind, 14, "NIP-17 chat message is kind:14");
    }

    #[test]
    fn build_dm_rumor_carries_sender_pubkey() {
        let input = DmInput {
            recipient_pubkey: RECIPIENT.to_string(),
            content: "hi".to_string(),
            reply_to: None,
        };
        let rumor = build_dm_rumor(&input, SENDER);
        assert_eq!(rumor.pubkey, SENDER, "rumor pubkey is the sender");
    }

    #[test]
    fn build_dm_rumor_carries_content_verbatim() {
        let input = DmInput {
            recipient_pubkey: RECIPIENT.to_string(),
            content: "the quick brown fox".to_string(),
            reply_to: None,
        };
        let rumor = build_dm_rumor(&input, SENDER);
        assert_eq!(rumor.content, "the quick brown fox");
    }

    #[test]
    fn build_dm_rumor_has_p_tag_for_recipient() {
        let input = DmInput {
            recipient_pubkey: RECIPIENT.to_string(),
            content: "hi".to_string(),
            reply_to: None,
        };
        let rumor = build_dm_rumor(&input, SENDER);
        assert_eq!(
            rumor.tags,
            vec![vec!["p".to_string(), RECIPIENT.to_string()]],
            "a non-reply rumor carries exactly one p-tag for the recipient"
        );
    }

    #[test]
    fn build_dm_rumor_adds_reply_e_tag_when_reply_to_set() {
        let parent = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccdd00";
        let input = DmInput {
            recipient_pubkey: RECIPIENT.to_string(),
            content: "replying".to_string(),
            reply_to: Some(parent.to_string()),
        };
        let rumor = build_dm_rumor(&input, SENDER);
        assert_eq!(
            rumor.tags,
            vec![
                vec!["p".to_string(), RECIPIENT.to_string()],
                vec![
                    "e".to_string(),
                    parent.to_string(),
                    String::new(),
                    "reply".to_string(),
                ],
            ],
            "a reply rumor carries the p-tag then a NIP-10 reply e-tag"
        );
    }

    #[test]
    fn build_dm_rumor_uses_created_at_zero_sentinel() {
        let input = DmInput {
            recipient_pubkey: RECIPIENT.to_string(),
            content: "hi".to_string(),
            reply_to: None,
        };
        let rumor = build_dm_rumor(&input, SENDER);
        assert_eq!(
            rumor.created_at, 0,
            "D7: created_at is the 0 sentinel — the actor re-stamps it"
        );
    }
}
