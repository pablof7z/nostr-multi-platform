//! `nmp.nip17.send` — the NIP-17 direct-message send [`ActionModule`].
//!
//! This is the typed action seam a host wires into the kernel's action
//! registry (`register_action_module` + `register_action_executor`) so a DM
//! send reaches the actor through the generic `dispatch_action` path — exactly
//! like the NIP-29 `post_chat_message` action.
//!
//! # Two halves
//!
//! * [`SendDmAction`] — the `ActionModule` *validator*. `start` is a pure
//!   shape check: non-empty content, non-empty recipient pubkey.
//! * [`send_dm_command`] — the *executor* function. It maps a validated
//!   [`SendDmInput`] to an [`ActorCommand::SendGiftWrappedDm`] carrying the
//!   kind:14 rumor. The actor's `send_gift_wrapped_dm` handler (local-keys
//!   MVP) does the seal + gift-wrap + publish.
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
        let rumor = build_dm_rumor(&dm_input, "");
        // Thread the registry-minted `correlation_id` onto the actor command so
        // the DM send participates in the action_results / action_stages round
        // trip — the actor records `Requested` on receipt and the per-envelope
        // `publish_signed_event` calls thread the id into the publish engine's
        // `correlation_id_override`. Without this, the host's spinner keyed on
        // the dispatched id would hang forever.
        send(ActorCommand::SendGiftWrappedDm {
            rumor,
            recipient_pubkey: action.recipient_pubkey,
            correlation_id: Some(correlation_id.to_string()),
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

}
