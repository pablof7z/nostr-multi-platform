use nmp_core::actor::{ActorCommand, PublishCommand};
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRegistrar,
    ActionRejection, ProtocolCommand, ProtocolCommandContext, ProtocolCommandError,
    ProtocolDescriptor,
};
use nmp_core::tags::{p_tag, q_tag};
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QuoteRepostAction {
    pub target_event_id: String,
    /// Nostr kind of the event being quoted. Quote reposts publish a kind:1
    /// note with a NIP-18 q tag; this kind is retained as target metadata.
    pub target_kind: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_author_pubkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_hint: Option<String>,
    pub content: String,
}

#[derive(Debug)]
pub struct RepostCommand {
    action: RepostAction,
    correlation_id: String,
}

#[derive(Debug)]
pub struct QuoteRepostCommand {
    action: QuoteRepostAction,
    correlation_id: String,
}

pub struct RepostModule;
pub struct QuoteRepostModule;

impl ActionModule for RepostModule {
    const NAMESPACE: nmp_core::substrate::DeclaredActionNamespace =
        nmp_core::substrate::DeclaredActionNamespace::framework(
            "nmp.nip18.repost",
            "action.nmp.nip18.repost",
        );
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

impl ActionModule for QuoteRepostModule {
    const NAMESPACE: nmp_core::substrate::DeclaredActionNamespace =
        nmp_core::substrate::DeclaredActionNamespace::framework(
            "nmp.nip18.quote_repost",
            "action.nmp.nip18.quote_repost",
        );
    type Action = QuoteRepostAction;

    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<QuoteRepostAction as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        validate_quote_repost(&action).map_err(ActionRejection::Invalid)
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(ActorCommand::Protocol(Box::new(QuoteRepostCommand {
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
        let event = build_repost_event(&self.action)
            .map_err(|err| ProtocolCommandError::new(format!("repost: {err}")))?;
        ctx.send(ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event,
            correlation_id: Some(self.correlation_id),
            signer_pubkey: None,
        }));
        Ok(())
    }
}

impl ProtocolCommand for QuoteRepostCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let event = quote_repost_event(&self.action)
            .map_err(|err| ProtocolCommandError::new(format!("quote repost: {err}")))?;
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
        app.register_default_action(QuoteRepostModule);
    }
}

pub fn register_actions(app: &mut impl ActionRegistrar) {
    app.register_default_action(RepostModule);
    app.register_default_action(QuoteRepostModule);
}

fn validate(action: &RepostAction) -> Result<(), String> {
    validate_target(
        &action.target_event_id,
        action.target_kind,
        action.target_author_pubkey.as_deref(),
        action.relay_hint.as_deref(),
        "repost",
    )
}

fn validate_quote_repost(action: &QuoteRepostAction) -> Result<(), String> {
    validate_target(
        &action.target_event_id,
        action.target_kind,
        action.target_author_pubkey.as_deref(),
        action.relay_hint.as_deref(),
        "quote repost",
    )?;
    if action.content.trim().is_empty() {
        return Err("quote repost requires non-empty content".to_string());
    }
    Ok(())
}

fn validate_target(
    target_event_id: &str,
    target_kind: u32,
    target_author_pubkey: Option<&str>,
    relay_hint: Option<&str>,
    label: &str,
) -> Result<(), String> {
    if !is_hex64(target_event_id) {
        return Err(format!("{label} requires a 64-hex target_event_id"));
    }
    if target_kind == 0 {
        return Err(format!("{label} requires a non-zero target_kind"));
    }
    if target_author_pubkey
        .map(str::trim)
        .is_some_and(|author| !is_hex64(author))
    {
        return Err(format!(
            "{label} target_author_pubkey must be 64-hex when provided"
        ));
    }
    if relay_hint
        .map(str::trim)
        .is_some_and(|relay| relay.is_empty())
    {
        return Err(format!(
            "{label} relay_hint must be non-empty when provided"
        ));
    }
    Ok(())
}

/// Build the bare NIP-18 repost event from its inputs — no routing, no
/// transport envelope. A `kind:1` target emits a `kind:6` repost wrapper;
/// every other non-zero target kind emits a `kind:16` generic repost. The
/// returned [`UnsignedEvent`] carries the `e` / `p` / `k` tags; `pubkey` /
/// `created_at` / `sig` are filled at sign time.
///
/// This is the **composition seam** for routing a repost *into* another
/// transport. To repost an event inside a NIP-29 group, build the repost here,
/// then hand its `(kind, content, tags)` to NIP-29's generic
/// `nmp.nip29.publish_group_event` surface, which injects only the
/// `h` / `previous` envelope. NIP-18 owns the `kind:6` / `kind:16`
/// construction; the transport owns only its envelope — NIP-29 never names,
/// classifies, or owns a repost kind (the kind-blind correction, #2513).
///
/// # Errors
///
/// Returns the validation message when `target_event_id` is not 64-hex,
/// `target_kind` is zero, or an optional `target_author_pubkey` / `relay_hint`
/// is malformed.
pub fn build_repost_event(action: &RepostAction) -> Result<UnsignedEvent, String> {
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

fn quote_repost_event(action: &QuoteRepostAction) -> Result<UnsignedEvent, String> {
    validate_quote_repost(action)?;
    let relay = action
        .relay_hint
        .as_deref()
        .map(str::trim)
        .filter(|relay| !relay.is_empty());

    let mut quote_tag = q_tag(action.target_event_id.trim(), relay);
    if let Some(pubkey) = action
        .target_author_pubkey
        .as_deref()
        .map(str::trim)
        .filter(|pubkey| !pubkey.is_empty())
    {
        if relay.is_none() {
            quote_tag.push(String::new());
        }
        quote_tag.push(pubkey.to_string());
    }

    let mut tags = vec![quote_tag];
    if let Some(pubkey) = action
        .target_author_pubkey
        .as_deref()
        .map(str::trim)
        .filter(|pubkey| !pubkey.is_empty())
    {
        tags.push(p_tag(pubkey, None));
    }
    tags.push(vec!["k".to_string(), action.target_kind.to_string()]);

    Ok(UnsignedEvent {
        pubkey: String::new(),
        kind: 1,
        tags,
        content: action.content.trim().to_string(),
        created_at: 0,
    })
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
#[path = "action_tests.rs"]
mod tests;
