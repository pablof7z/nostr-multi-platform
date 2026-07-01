use nmp_core::actor::{ActorCommand, PublishCommand};
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionReadError,
    ActionRegistrar, ActionRejection, KernelEvent, ProtocolCommand, ProtocolCommandContext,
    ProtocolCommandError,
};
use nmp_kinds::{KIND_NIP22_COMMENT, KIND_SHORT_TEXT_NOTE};
use nmp_nip01::try_from_kernel_event as note_from_kernel_event;
use nmp_nip22::try_from_kernel_event as comment_from_kernel_event;
use nmp_store::StoredEvent;
use serde::{Deserialize, Serialize};

use crate::build::Reply;
use crate::target::{is_hex64, trimmed_optional, ReplyTarget};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReplyAction {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_event_id: Option<String>,
    #[serde(default)]
    pub target_kind: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_author_pubkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_external_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_hint: Option<String>,
    pub content: String,
}

#[derive(Debug)]
pub struct ReplyCommand {
    target: ReplyTarget,
    content: String,
    relay_hint: Option<String>,
    correlation_id: String,
}

pub struct ReplyModule;

impl ActionModule for ReplyModule {
    const NAMESPACE: nmp_core::substrate::DeclaredActionNamespace =
        nmp_core::substrate::DeclaredActionNamespace::framework(
            "nmp.replies.reply",
            "action.nmp.replies.reply",
        );
    type Action = ReplyAction;

    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<ReplyAction as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        validate(&action).map_err(ActionRejection::Invalid)
    }

    fn execute(
        &self,
        ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let target = resolve_action_target(ctx, &action)?;
        send(ActorCommand::Protocol(Box::new(ReplyCommand {
            target,
            content: action.content,
            relay_hint: trimmed_optional(action.relay_hint),
            correlation_id: correlation_id.to_string(),
        })));
        Ok(())
    }
}

impl ProtocolCommand for ReplyCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let mut builder = Reply::to(self.target, self.content);
        if let Some(relay) = self.relay_hint {
            builder = builder.relay_hint(relay);
        }
        let event = builder
            .build(String::new(), 0)
            .map_err(|err| ProtocolCommandError::new(format!("reply: {err}")))?;
        ctx.send(ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event,
            correlation_id: Some(self.correlation_id),
            signer_pubkey: None,
        }));
        Ok(())
    }
}

pub fn register_actions(app: &mut impl ActionRegistrar) {
    app.register_default_action(ReplyModule);
}

fn validate(action: &ReplyAction) -> Result<(), String> {
    if action.content.trim().is_empty() {
        return Err("reply content must not be empty".to_string());
    }
    let target_count = [
        action.target_event_id.as_deref(),
        action.target_address.as_deref(),
        action.target_external_uri.as_deref(),
    ]
    .into_iter()
    .filter(|value| value.is_some_and(|v| !v.trim().is_empty()))
    .count();
    if target_count != 1 {
        return Err("reply action requires exactly one target".to_string());
    }
    if let Some(event_id) = action.target_event_id.as_deref().map(str::trim) {
        if !event_id.is_empty() && !is_hex64(event_id) {
            return Err("target_event_id must be 64-hex when provided".to_string());
        }
    }
    if action
        .target_author_pubkey
        .as_deref()
        .map(str::trim)
        .is_some_and(|author| !author.is_empty() && !is_hex64(author))
    {
        return Err("target_author_pubkey must be 64-hex when provided".to_string());
    }
    Ok(())
}

fn resolve_action_target(ctx: &ActionContext, action: &ReplyAction) -> Result<ReplyTarget, String> {
    if let Some(event_id) = action
        .target_event_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        return resolve_event_target(ctx, action, event_id);
    }
    if let Some(address) = action
        .target_address
        .as_deref()
        .map(str::trim)
        .filter(|addr| !addr.is_empty())
    {
        return ReplyTarget::address(
            address,
            action.target_kind,
            trimmed_optional(action.target_author_pubkey.clone()),
        )
        .map_err(|err| err.to_string());
    }
    if let Some(uri) = action
        .target_external_uri
        .as_deref()
        .map(str::trim)
        .filter(|uri| !uri.is_empty())
    {
        return ReplyTarget::external(uri).map_err(|err| err.to_string());
    }
    Err("reply action requires a target".to_string())
}

fn resolve_event_target(
    ctx: &ActionContext,
    action: &ReplyAction,
    event_id: &str,
) -> Result<ReplyTarget, String> {
    match ctx.local_event_by_id(event_id) {
        Ok(Some(stored)) => {
            let event = stored_to_kernel_event(&stored);
            if let Some(note) = note_from_kernel_event(&event) {
                return Ok(ReplyTarget::note(note));
            }
            if let Some(comment) = comment_from_kernel_event(&event) {
                return Ok(ReplyTarget::comment(comment));
            }
            return ReplyTarget::event(event.id, event.kind, Some(event.author))
                .map_err(|err| err.to_string());
        }
        Ok(None) | Err(ActionReadError::StoreUnavailable) => {}
        Err(err) => return Err(format!("reply target local read failed: {err}")),
    }

    if action.target_kind == KIND_NIP22_COMMENT {
        return Err("replying to a kind:1111 comment requires the local comment event".to_string());
    }
    if action.target_kind == KIND_SHORT_TEXT_NOTE
        && trimmed_optional(action.target_author_pubkey.clone()).is_none()
    {
        return Err(
            "replying to a kind:1 note requires target_author_pubkey when the event is not local"
                .to_string(),
        );
    }
    ReplyTarget::event(
        event_id,
        action.target_kind,
        trimmed_optional(action.target_author_pubkey.clone()),
    )
    .map_err(|err| err.to_string())
}

fn stored_to_kernel_event(stored: &StoredEvent) -> KernelEvent {
    let raw = stored.raw.as_ref();
    KernelEvent {
        id: raw.id.clone(),
        author: raw.pubkey.clone(),
        kind: raw.kind,
        created_at: raw.created_at,
        tags: raw.tags.clone(),
        content: raw.content.clone(),
        relay_provenance: Vec::new(),
    }
}

#[cfg(test)]
#[path = "action_tests.rs"]
mod tests;
