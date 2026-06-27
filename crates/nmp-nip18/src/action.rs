use nmp_core::actor::{ActorCommand, PublishCommand};
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRegistrar,
    ActionRejection, ProtocolCommand, ProtocolCommandContext, ProtocolCommandError,
    ProtocolDescriptor,
};
use nmp_signer_iface::UnsignedEvent;
use serde::{Deserialize, Serialize};

use crate::{KIND_GENERIC_REPOST, KIND_REPOST};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RepostAction {
    pub target_event_id: String,
    /// Nostr kind of the event being reposted. Kind:1 emits a NIP-18 kind:6
    /// repost wrapper; every other non-zero kind emits a kind:16 generic repost.
    pub target_kind: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_author_pubkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_hint: Option<String>,
}

#[derive(Debug)]
pub struct RepostCommand {
    action: RepostAction,
    correlation_id: String,
}

pub struct RepostModule;

impl ActionModule for RepostModule {
    const NAMESPACE: &'static str = "nmp.nip18.repost";
    type Action = RepostAction;

    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<RepostAction as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        validate(&action).map_err(ActionRejection::Invalid)
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(ActorCommand::Protocol(Box::new(RepostCommand {
            action,
            correlation_id: correlation_id.to_string(),
        })));
        Ok(())
    }
}

impl ProtocolCommand for RepostCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let event = repost_event(&self.action)
            .map_err(|err| ProtocolCommandError::new(format!("repost: {err}")))?;
        ctx.send(ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event,
            correlation_id: Some(self.correlation_id),
            signer_pubkey: None,
        }));
        Ok(())
    }
}

pub struct Nip18Descriptor;

impl ProtocolDescriptor for Nip18Descriptor {
    fn register_actions(&self, app: &mut impl ActionRegistrar) {
        app.register_default_action(RepostModule);
    }
}

pub fn register_actions(app: &mut impl ActionRegistrar) {
    app.register_default_action(RepostModule);
}

fn validate(action: &RepostAction) -> Result<(), String> {
    if !is_hex64(&action.target_event_id) {
        return Err("repost requires a 64-hex target_event_id".to_string());
    }
    if action.target_kind == 0 {
        return Err("repost requires a non-zero target_kind".to_string());
    }
    if action
        .target_author_pubkey
        .as_deref()
        .map(str::trim)
        .is_some_and(|author| !is_hex64(author))
    {
        return Err("repost target_author_pubkey must be 64-hex when provided".to_string());
    }
    if action
        .relay_hint
        .as_deref()
        .map(str::trim)
        .is_some_and(|relay| relay.is_empty())
    {
        return Err("repost relay_hint must be non-empty when provided".to_string());
    }
    Ok(())
}

fn repost_event(action: &RepostAction) -> Result<UnsignedEvent, String> {
    validate(action)?;
    let target_id = action.target_event_id.trim().to_string();
    let mut e_tag = vec!["e".to_string(), target_id];
    if let Some(relay) = action
        .relay_hint
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
    {
        e_tag.push(relay.to_string());
    }

    let mut tags = vec![e_tag];
    if let Some(pubkey) = action
        .target_author_pubkey
        .as_deref()
        .map(str::trim)
        .filter(|pubkey| !pubkey.is_empty())
    {
        tags.push(vec!["p".to_string(), pubkey.to_string()]);
    }
    tags.push(vec!["k".to_string(), action.target_kind.to_string()]);

    Ok(UnsignedEvent {
        pubkey: String::new(),
        kind: if action.target_kind == 1 {
            KIND_REPOST
        } else {
            KIND_GENERIC_REPOST
        },
        tags,
        content: String::new(),
        created_at: 0,
    })
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
#[path = "action_tests.rs"]
mod tests;
