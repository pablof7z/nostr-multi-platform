use nmp_core::actor::ActorCommand;
use nmp_core::actor::PublishCommand;
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRegistrar,
    ActionRejection, ProtocolCommand, ProtocolCommandContext, ProtocolCommandError,
    ProtocolDescriptor,
};
use nmp_signer_iface::UnsignedEvent;
use serde::{Deserialize, Serialize};

pub const KIND_REACTION: u32 = 7;
pub const KIND_REACTION_DELETE: u32 = 5;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReactAction {
    pub target_event_id: String,
    #[serde(default = "default_reaction")]
    pub reaction: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_author_pubkey: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UnreactAction {
    pub reaction_event_id: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug)]
pub struct PublishReactionCommand {
    action: ReactAction,
    correlation_id: String,
}

#[derive(Debug)]
pub struct UnreactReactionCommand {
    action: UnreactAction,
    correlation_id: String,
}

pub struct ReactModule;
pub struct UnreactModule;

impl ActionModule for ReactModule {
    const NAMESPACE: &'static str = "nmp.nip25.react";
    type Action = ReactAction;

    /// ADR-0064 / S3: opt into the typed FlatBuffers payload doorway; the
    /// fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<ReactAction as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        validate_react(&action)
    }

    fn execute(
        &self,
        ctx: &ActionContext,
        mut action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        action.target_author_pubkey = resolve_target_author_pubkey(ctx, &action);
        send(ActorCommand::Protocol(Box::new(PublishReactionCommand {
            action,
            correlation_id: correlation_id.to_string(),
        })));
        Ok(())
    }
}

impl ActionModule for UnreactModule {
    const NAMESPACE: &'static str = "nmp.nip25.unreact";
    type Action = UnreactAction;

    /// ADR-0064 / S3: opt into the typed FlatBuffers payload doorway; the
    /// fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<UnreactAction as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        if !is_hex64(&action.reaction_event_id) {
            return Err(ActionRejection::Invalid(
                "unreact requires a 64-hex reaction_event_id".to_string(),
            ));
        }
        Ok(())
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(ActorCommand::Protocol(Box::new(UnreactReactionCommand {
            action,
            correlation_id: correlation_id.to_string(),
        })));
        Ok(())
    }
}

impl ProtocolCommand for PublishReactionCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let event = build_reaction_event(&self.action)
            .map_err(|err| ProtocolCommandError::new(format!("react: {err}")))?;
        ctx.send(ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event,
            correlation_id: Some(self.correlation_id),
            signer_pubkey: None,
        }));
        Ok(())
    }
}

impl ProtocolCommand for UnreactReactionCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        ctx.send(ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event: UnsignedEvent {
                pubkey: String::new(),
                kind: KIND_REACTION_DELETE,
                tags: vec![vec!["e".to_string(), self.action.reaction_event_id]],
                content: self.action.reason,
                created_at: 0,
            },
            correlation_id: Some(self.correlation_id),
            signer_pubkey: None,
        }));
        Ok(())
    }
}

/// Typed protocol descriptor for NIP-25 reactions (#1724 criterion 5).
///
/// Zero-cost unit struct exposing this crate's two action-module contributions
/// (`nmp.nip25.react`, `nmp.nip25.unreact`) through the [`ProtocolDescriptor`]
/// trait so `nmp-defaults` can compose descriptors rather than call ad-hoc
/// `register_actions` free functions (criterion 6).
///
/// Both modules are registered as **yielding defaults** (ADR-0049 Part 1): an
/// app that pre-registers its own reaction handler pre-empts these regardless of
/// call order.
pub struct Nip25Descriptor;

impl ProtocolDescriptor for Nip25Descriptor {
    fn register_actions(&self, app: &mut impl ActionRegistrar) {
        app.register_default_action(ReactModule);
        app.register_default_action(UnreactModule);
    }
}

fn validate_react(action: &ReactAction) -> Result<(), ActionRejection> {
    if !is_hex64(&action.target_event_id) {
        return Err(ActionRejection::Invalid(
            "react requires a 64-hex target_event_id".to_string(),
        ));
    }
    if action
        .target_author_pubkey
        .as_deref()
        .is_some_and(|author| !is_hex64(author))
    {
        return Err(ActionRejection::Invalid(
            "react target_author_pubkey must be 64-hex when provided".to_string(),
        ));
    }
    Ok(())
}

/// Build the bare NIP-25 reaction (`kind:7`) event from its inputs — no
/// routing, no transport envelope. The returned [`UnsignedEvent`] carries the
/// `e`/`p` reaction tags and content; `pubkey` / `created_at` / `sig` are
/// filled at sign time.
///
/// This is the **composition seam** for routing a reaction *into* another
/// transport. To react to an event inside a NIP-29 group, build the reaction
/// here, then hand its `(kind, content, tags)` to NIP-29's generic
/// `nmp.nip29.publish_group_event` surface (`PublishGroupEventInput`), which
/// injects only the `h` / `previous` envelope. NIP-25 owns the `kind:7`
/// construction; the transport owns only its envelope — NIP-29 never names,
/// classifies, or owns `kind:7` (the #2504/#2505 kind-blind correction, #2513).
///
/// # Errors
///
/// Returns the validation message when `target_event_id` is not 64-hex or
/// `target_author_pubkey` is supplied but malformed.
pub fn build_reaction_event(action: &ReactAction) -> Result<UnsignedEvent, String> {
    match validate_react(action) {
        Ok(()) => {}
        Err(ActionRejection::Invalid(msg)) => return Err(msg),
        Err(other) => return Err(format!("{other:?}")),
    }
    let (tags, content) = reaction_tags(action)
        .ok_or_else(|| "react requires a 64-hex target_event_id".to_string())?;
    Ok(UnsignedEvent {
        pubkey: String::new(),
        kind: KIND_REACTION,
        tags,
        content,
        created_at: 0,
    })
}

fn reaction_tags(action: &ReactAction) -> Option<(Vec<Vec<String>>, String)> {
    if !is_hex64(&action.target_event_id) {
        return None;
    }
    let content = if action.reaction.trim().is_empty() {
        "+".to_string()
    } else {
        action.reaction.clone()
    };
    let mut tags = vec![vec!["e".to_string(), action.target_event_id.clone()]];
    if let Some(author) = &action.target_author_pubkey {
        tags.push(vec!["p".to_string(), author.clone()]);
    }
    Some((tags, content))
}

fn resolve_target_author_pubkey(ctx: &ActionContext, action: &ReactAction) -> Option<String> {
    if action.target_author_pubkey.is_some() {
        return action.target_author_pubkey.clone();
    }
    ctx.local_event_by_id(&action.target_event_id)
        .ok()
        .flatten()
        .map(|stored| stored.raw.pubkey.clone())
        .filter(|pubkey| is_hex64(pubkey))
}

fn default_reaction() -> String {
    "+".to_string()
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
#[path = "action_tests.rs"]
mod tests;
