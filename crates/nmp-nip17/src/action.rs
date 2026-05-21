//! `nmp.nip17.send` — the NIP-17 direct-message send [`ActionModule`].
//!
//! This is the typed action seam a host wires into the kernel's action
//! registry via [`nmp_core::NmpApp::register_action_module`] so a DM send
//! reaches the actor through the generic `dispatch_action` path — exactly
//! like the NIP-29 `post_chat_message` action.
//!
//! # ADR-0027 — one trait, two halves
//!
//! [`SendDmAction`] is the typed [`ActionModule`]. Its `start` is a pure
//! shape validator (non-empty content + recipient pubkey); its `execute`
//! builds the kind:14 rumor and enqueues [`ActorCommand::SendGiftWrappedDm`].
//! Before ADR-0027 the two halves lived in separate symbols
//! (`SendDmAction` + the free function `send_dm_command`); now they live
//! on the same trait impl.
//!
//! # Why the rumor's `pubkey` is left empty
//!
//! The action module runs on the FFI thread and does not know the active
//! account's pubkey — only the actor does. [`build_dm_rumor`] takes a
//! `sender_pubkey` argument, but the actor's `dm.rs::build_nostr_rumor`
//! re-derives the pubkey from the signing `Keys` (`EventBuilder::build(pubkey)`
//! takes the pubkey separately and ignores `rumor.pubkey`). So passing an
//! empty string here is correct — the actor overrides it at sign time. This
//! mirrors the NIP-29 actions, whose `event.pubkey` is likewise a placeholder.

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

use crate::{build_dm_rumor, DmInput};

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

    /// Map a validated [`SendDmInput`] to the
    /// [`ActorCommand::SendGiftWrappedDm`] that drives the actor's DM
    /// handler. The carried rumor is built with an empty `sender_pubkey`
    /// placeholder — the actor re-derives the real pubkey from the signing
    /// key (see the module docs). `created_at` is the D7 `0` sentinel; the
    /// actor re-stamps it from the kernel clock before wrapping.
    fn execute(
        action: Self::Action,
        _correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let dm_input = DmInput {
            recipient_pubkey: action.recipient_pubkey.clone(),
            content: action.content,
            reply_to: action.reply_to,
        };
        let rumor = build_dm_rumor(&dm_input, "");
        send(ActorCommand::SendGiftWrappedDm {
            rumor,
            recipient_pubkey: action.recipient_pubkey,
        });
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

    /// Capture every `ActorCommand` an `M::execute` call sends through.
    fn capture_execute(input: SendDmInput) -> Vec<ActorCommand> {
        use std::cell::RefCell;
        let cell: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        SendDmAction::execute(input, "test-corr-id", &|cmd| {
            cell.borrow_mut().push(cmd);
        })
        .expect("well-formed");
        cell.into_inner()
    }

    #[test]
    fn execute_builds_a_send_gift_wrapped_dm() {
        let cmds = capture_execute(SendDmInput {
            recipient_pubkey: RECIPIENT.to_string(),
            content: "hello there".to_string(),
            reply_to: None,
        });
        assert_eq!(cmds.len(), 1, "execute must enqueue exactly one ActorCommand");
        match cmds.into_iter().next().unwrap() {
            ActorCommand::SendGiftWrappedDm {
                rumor,
                recipient_pubkey,
            } => {
                assert_eq!(rumor.kind, 14, "the carried rumor is kind:14");
                assert_eq!(rumor.content, "hello there");
                assert_eq!(recipient_pubkey, RECIPIENT);
                // D7: the rumor carries the `0` sentinel — the actor re-stamps.
                assert_eq!(rumor.created_at, 0);
                assert!(rumor.pubkey.is_empty());
                assert!(rumor
                    .tags
                    .iter()
                    .any(|t| t == &vec!["p".to_string(), RECIPIENT.to_string()]));
            }
            other => panic!("expected SendGiftWrappedDm, got {other:?}"),
        }
    }

    #[test]
    fn execute_carries_the_reply_marker() {
        let parent =
            "cc11223344556677889900aabbccddeeff00112233445566778899aabbccdd00";
        let cmds = capture_execute(SendDmInput {
            recipient_pubkey: RECIPIENT.to_string(),
            content: "re".to_string(),
            reply_to: Some(parent.to_string()),
        });
        match cmds.into_iter().next().unwrap() {
            ActorCommand::SendGiftWrappedDm { rumor, .. } => {
                assert!(
                    rumor.tags.iter().any(|t| t
                        == &vec![
                            "e".to_string(),
                            parent.to_string(),
                            String::new(),
                            "reply".to_string(),
                        ]),
                    "a reply DM carries the NIP-10 reply e-tag, got {:?}",
                    rumor.tags
                );
            }
            other => panic!("expected SendGiftWrappedDm, got {other:?}"),
        }
    }
}
