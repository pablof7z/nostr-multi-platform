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
//! # Scope of this crate
//!
//! Three concerns, mirroring the NIP-17 lifecycle:
//!
//! * **Send (rumor build)** — [`build_dm_rumor`] turns a [`DmInput`] into an
//!   [`UnsignedEvent`] (kind:14). This carries no key material and performs no
//!   crypto; the gift-wrap and publish happen on the actor thread (D7 — the
//!   kernel owns key access and the wall clock), driven by
//!   `ActorCommand::SendGiftWrappedDm`.
//! * **Send (action)** — [`action::SendDmAction`] is the `ActionModule` a host
//!   wires into the kernel's action registry so `nmp.nip17.send` reaches the
//!   actor through the generic `dispatch_action` path.
//! * **Receive** — [`inbox::DmInboxProjection`] is the `RawEventObserver` that
//!   taps kind:1059 gift-wraps, unseals them with the active account's local
//!   keys, and projects the decrypted conversation list. Crypto here is the
//!   NIP-44 unseal inside `nmp_nip59::unwrap_gift_wrap`.
//!
//! The actor's `SendGiftWrappedDm` arm is a **local-keys-only MVP**: a remote
//! (NIP-46 / bunker) signer cannot gift-wrap because `nmp_nip59::gift_wrap`
//! needs raw `nostr::Keys` for the NIP-44 seal. The `RemoteSignerHandle`
//! NIP-44 seam (ADR-0026) is built — wiring it into the seal step is the
//! bunker-DM phase. This rumor-builder crate is signer-agnostic and unaffected
//! by that gap.
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

use nmp_core::substrate::{AppHost, UnsignedEvent};

pub mod action;
pub mod dm_relay_cache;
pub mod dm_relay_list;
pub mod dm_runtime;
pub mod dm_send;
pub mod inbox;
pub mod kind10050_parser;

pub use action::{SendDmAction, SendDmInput};
pub use dm_relay_cache::DmRelayCache;
pub use dm_relay_list::{
    build_dm_relay_list_event, PublishDmRelayListAction, PublishDmRelayListInput,
};
pub use dm_runtime::{DmRuntimeEffect, DmRuntimeState};
pub use dm_send::SendGiftWrappedDmCommand;
pub use inbox::{
    active_giftwrap_inbox_interest, active_giftwrap_inbox_interest_id, DmConversation,
    DmInboxProjection, DmInboxSnapshot, DmMessage,
};
pub use kind10050_parser::Kind10050Parser;

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
#[must_use] 
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

/// Register every NIP-17 substrate seam against `app`:
///
/// 1. The two [`nmp_core::substrate::ActionModule`] verbs
///    (`nmp.nip17.send` and `nmp.nip17.publish_relay_list`).
/// 2. The kind:10050 [`Kind10050Parser`] (V-40) — wired into the
///    kernel's [`nmp_core::substrate::EventIngestDispatcher`] so an
///    accepted kind:10050 writes the `DmRelayCache`.
/// 3. The `DmRelayCache` itself (V-40) — installed as the kernel's
///    `Arc<dyn DmInboxRelayLookup>` so the gift-wrap publish path's
///    `recipient_dm_relays` reader + the planner's `#p`-tagged inbox
///    routing both see the same entries this crate writes.
///
/// Production composition calls this once at app startup. Repeated
/// calls re-register the parser additively (the dispatcher allows
/// multiple parsers per kind by design); the `Arc<DmRelayCache>` is
/// swapped each call, so callers that need a stable handle should
/// store the cache themselves and pass it explicitly. The default
/// path (one composition, one cache) is the common case.
pub fn register_actions(app: &mut impl AppHost) {
    app.register_action::<SendDmAction>();
    app.register_action::<PublishDmRelayListAction>();

    // V-40 — install the shared `DmRelayCache` on both ends:
    //   1. As the kernel's `Arc<dyn DmInboxRelayLookup>` (reader).
    //   2. As the `Kind10050Parser`'s backing cache (writer).
    let cache = std::sync::Arc::new(DmRelayCache::new());
    let as_lookup: std::sync::Arc<dyn nmp_core::substrate::DmInboxRelayLookup> =
        std::sync::Arc::clone(&cache) as _;
    app.set_dm_inbox_relay_lookup(as_lookup);
    let parser: std::sync::Arc<dyn nmp_core::substrate::IngestParser> =
        std::sync::Arc::new(Kind10050Parser::new(cache)) as _;
    app.register_ingest_parser(10_050, parser);
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
