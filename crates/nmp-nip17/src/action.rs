//! `nmp.nip17.send` — the NIP-17 direct-message send [`ActionModule`].
//!
//! This is the typed action seam a host wires into the kernel's action
//! registry (`ActionRegistry::register::<SendDmAction>()`) so a DM send
//! reaches the actor through the generic `dispatch_action` path — exactly
//! like the NIP-29 `post_chat_message` action.
//!
//! # Two halves
//!
//! * [`SendDmAction`] — the `ActionModule` *validator*. `start` is a pure
//!   shape check: non-empty content, non-empty recipient pubkey.
//! * The executor maps a validated [`SendDmInput`] to an
//!   `ActorCommand::Protocol(Box::new(SendGiftWrappedDmCommand{...}))`
//!   (V-39) carrying the kind:14 rumor. The protocol-command body —
//!   [`crate::dm_send::SendGiftWrappedDmCommand`] in this crate — runs on
//!   the actor thread and does the seal + gift-wrap + publish chain.
//!
//! # Why the rumor's `pubkey` is empty
//!
//! The action module runs on the FFI thread and does not know the active
//! account's pubkey — only the actor does. [`build_dm_rumor`] sets `pubkey`
//! to `""` internally (D7 sentinel). The actor's `dm.rs::build_nostr_rumor`
//! re-derives the pubkey from the signing `Keys` at gift-wrap time, exactly
//! as the NIP-29 actions do.

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

use crate::{build_dm_rumor, DmInput, SendGiftWrappedDmCommand};

/// Wire shape for `nmp.nip17.send` — the JSON a host passes to
/// `nmp_app_dispatch_action`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SendDmInput {
    /// Recipient's Nostr public key, lowercase hex.
    pub recipient_pubkey: String,
    /// The plaintext message body — becomes the kind:14 `content`.
    pub content: String,
    /// Optional event id (hex) this message replies to. When set, the rumor
    /// carries a NIP-10 `["e", reply_to, "", "reply"]` marker.
    #[serde(default)]
    pub reply_to: Option<String>,
}

/// The `nmp.nip17.send` [`ActionModule`] — a pure shape validator.
pub struct SendDmAction;

impl ActionModule for SendDmAction {
    const NAMESPACE: &'static str = "nmp.nip17.send";
    type Action = SendDmInput;

    /// Validate a DM send request. `start` carries no side effects: it only
    /// rejects an empty body or a missing recipient. The actual seal /
    /// gift-wrap / publish — and the recipient-pubkey *parse* — happen on the
    /// actor thread (which owns the user-facing error toasts, D6).
    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection> {
        if action.content.trim().is_empty() {
            return Err(ActionRejection::Invalid("empty DM content".into()));
        }
        if action.recipient_pubkey.trim().is_empty() {
            return Err(ActionRejection::Invalid(
                "missing recipient pubkey".into(),
            ));
        }
        Ok(())
    }
    fn execute(
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let dm_input = DmInput {
            recipient_pubkey: action.recipient_pubkey.clone(),
            content: action.content,
            reply_to: action.reply_to,
        };
        let rumor = build_dm_rumor(&dm_input);
        // V-39: dispatch via the substrate `ActorCommand::Protocol` arm
        // wrapping a `SendGiftWrappedDmCommand`. The protocol-command
        // body runs on the actor thread, resolves the active local
        // signer + the recipient's kind:10050 list through the
        // `ProtocolCommandContext`, gift-wraps the rumor twice, and
        // dispatches each kind:1059 envelope back through
        // `ctx.send(ActorCommand::PublishSignedEvent { ... })`. The
        // `correlation_id` threads onto every follow-up so the publish
        // engine's terminal verdict clears the host's spinner.
        send(ActorCommand::Protocol(Box::new(SendGiftWrappedDmCommand {
            rumor,
            recipient_pubkey: action.recipient_pubkey,
            correlation_id: Some(correlation_id.to_string()),
        })));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECIPIENT: &str =
        "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";

    fn ctx() -> ActionContext {
        ActionContext::default()
    }

    #[test]
    fn namespace_is_nmp_nip17_send() {
        assert_eq!(SendDmAction::NAMESPACE, "nmp.nip17.send");
    }

    #[test]
    fn start_accepts_a_well_formed_dm() {
        let input = SendDmInput {
            recipient_pubkey: RECIPIENT.to_string(),
            content: "hello".to_string(),
            reply_to: None,
        };
        assert!(SendDmAction::start(&mut ctx(), input).is_ok());
    }

    #[test]
    fn start_rejects_empty_content() {
        let input = SendDmInput {
            recipient_pubkey: RECIPIENT.to_string(),
            content: "   ".to_string(),
            reply_to: None,
        };
        assert!(matches!(
            SendDmAction::start(&mut ctx(), input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn start_rejects_missing_recipient() {
        let input = SendDmInput {
            recipient_pubkey: String::new(),
            content: "hi".to_string(),
            reply_to: None,
        };
        assert!(matches!(
            SendDmAction::start(&mut ctx(), input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn execute_emits_protocol_send_gift_wrapped_dm_with_correct_fields() {
        use nmp_core::ActorCommand;
        use std::cell::RefCell;

        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        let input = SendDmInput {
            recipient_pubkey: RECIPIENT.to_string(),
            content: "hello world".to_string(),
            reply_to: None,
        };
        SendDmAction::execute(input, "cid-dm", &|cmd| {
            captured.borrow_mut().push(cmd);
        })
        .expect("well-formed input executes");
        let cmds = captured.into_inner();
        assert_eq!(cmds.len(), 1, "executor must send exactly one command, got {cmds:?}");
        match cmds.into_iter().next().unwrap() {
            ActorCommand::Protocol(boxed) => {
                // The boxed `ProtocolCommand`'s debug repr carries the
                // recipient and content. We assert through `Debug` so the
                // boxed-dyn shape is not coupled to a concrete downcast
                // (the action lives in `nmp-nip17` and the command type
                // is local — but the dispatch shape MUST be the
                // substrate-generic `Protocol(Box<dyn ProtocolCommand>)`).
                let s = format!("{boxed:?}");
                assert!(
                    s.contains("SendGiftWrappedDmCommand"),
                    "Protocol-arm payload must be a SendGiftWrappedDmCommand; got: {s}"
                );
                assert!(s.contains(RECIPIENT), "recipient must round-trip");
                assert!(s.contains("hello world"), "DM content must land in the rumor");
                assert!(s.contains("cid-dm"), "correlation_id must thread through");
            }
            other => panic!("expected ActorCommand::Protocol, got {other:?}"),
        }
    }

}
