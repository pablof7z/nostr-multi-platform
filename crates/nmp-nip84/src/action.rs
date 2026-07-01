use nmp_core::actor::ActorCommand;
use nmp_core::actor::PublishCommand;
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRegistrar,
    ActionRejection, KernelEvent, ProtocolCommand, ProtocolCommandContext, ProtocolCommandError,
    ProtocolDescriptor,
};
use nmp_signer_iface::UnsignedEvent;
use serde::{Deserialize, Serialize};

use crate::external_id;

/// NIP-84 highlight event kind.
pub const KIND_HIGHLIGHT: u32 = 9802;

/// A user intent to publish a NIP-84 kind:9802 highlight.
///
/// The highlighted text is the event `content`; every other field maps to an
/// optional tag (`alt`, `e`, `a`, `p`, `context`) or, for [`Self::external_ids`],
/// to NIP-73 `i` tags plus derived `k` tags for external content identifiers.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PublishHighlightAction {
    /// The highlighted text (kind:9802 content).
    pub content: String,
    /// Surrounding context for the highlight (emitted as `context` tag when present).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// Source Nostr event id (hex-64) to tag with `e`. Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_id: Option<String>,
    /// Source addressable event as `<kind>:<pubkey>:<d-identifier>` to tag with `a`. Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_address: Option<String>,
    /// Pubkey (hex-64) of the source author to tag with `p`. Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_author_pubkey: Option<String>,
    /// Human-readable alt description (emitted as `alt` tag when present).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
    /// NIP-73 external content identifiers (e.g. `"podcast:item:guid:<guid>"`,
    /// `"https://example.com/article"`). Each emitted as an `i` tag. The
    /// corresponding deduplicated `k` tags are derived from these values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_ids: Vec<String>,
}

/// Owner-provided render adapter for an embedded NIP-84 kind:9802 highlight.
///
/// Rendering crates may adapt this into their wire envelope, but the NIP-84
/// content/tag semantics stay in this crate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HighlightProjection {
    pub id: String,
    pub author_pubkey: String,
    pub created_at: u64,
    pub highlighted_text: String,
    pub source_event_id: Option<String>,
    pub source_event_addr: Option<String>,
    pub source_url: Option<String>,
    pub context: Option<String>,
}

#[derive(Debug)]
pub struct PublishHighlightCommand {
    action: PublishHighlightAction,
    correlation_id: String,
}

/// `nmp.nip84.publish_highlight` action module.
pub struct PublishHighlightModule;

impl ActionModule for PublishHighlightModule {
    const NAMESPACE: &'static str = "nmp.nip84.publish_highlight";
    type Action = PublishHighlightAction;

    /// Opt into the typed payload doorway; the fail-closed `schema_version`
    /// gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<PublishHighlightAction as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        validate_highlight(&action)
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(ActorCommand::Protocol(Box::new(PublishHighlightCommand {
            action,
            correlation_id: correlation_id.to_string(),
        })));
        Ok(())
    }
}

impl ProtocolCommand for PublishHighlightCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Some(tags) = highlight_tags(&self.action) else {
            return Err(ProtocolCommandError::new(
                "publish_highlight: malformed highlight fields",
            ));
        };
        ctx.send(ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event: UnsignedEvent {
                pubkey: String::new(),
                kind: KIND_HIGHLIGHT,
                tags,
                content: self.action.content,
                created_at: 0,
            },
            correlation_id: Some(self.correlation_id),
            signer_pubkey: None,
        }));
        Ok(())
    }
}

/// Typed protocol descriptor for the NIP-84 highlight publish action.
///
/// Zero-cost unit struct exposing this crate's single action-module
/// contribution (`nmp.nip84.publish_highlight`) through the
/// [`ProtocolDescriptor`] trait so `explicit composition` can compose descriptors
/// rather than call ad-hoc `register_actions` free functions.
///
/// Registered as a **yielding default**: an app that pre-registers its own
/// highlight handler pre-empts this regardless of call order.
pub struct Nip84Descriptor;

impl ProtocolDescriptor for Nip84Descriptor {
    fn register_actions(&self, app: &mut impl ActionRegistrar) {
        app.register_default_action(PublishHighlightModule);
    }
}

fn validate_highlight(action: &PublishHighlightAction) -> Result<(), ActionRejection> {
    if action.content.is_empty() {
        return Err(ActionRejection::Invalid(
            "publish_highlight requires non-empty content".to_string(),
        ));
    }
    if action
        .source_event_id
        .as_deref()
        .is_some_and(|id| !is_hex64(id))
    {
        return Err(ActionRejection::Invalid(
            "publish_highlight source_event_id must be 64-hex when provided".to_string(),
        ));
    }
    if action
        .source_author_pubkey
        .as_deref()
        .is_some_and(|pk| !is_hex64(pk))
    {
        return Err(ActionRejection::Invalid(
            "publish_highlight source_author_pubkey must be 64-hex when provided".to_string(),
        ));
    }
    if action
        .external_ids
        .iter()
        .any(|id| external_id::kind_for_id(id).is_none())
    {
        return Err(ActionRejection::Invalid(
            "publish_highlight external_ids must be recognized NIP-73 identifiers".to_string(),
        ));
    }
    Ok(())
}

#[must_use]
pub fn highlight_projection_from_event(event: &KernelEvent) -> Option<HighlightProjection> {
    if event.kind != KIND_HIGHLIGHT {
        return None;
    }
    Some(HighlightProjection {
        id: event.id.clone(),
        author_pubkey: event.author.clone(),
        created_at: event.created_at,
        highlighted_text: event.content.clone(),
        source_event_id: tag_value(&event.tags, "e"),
        source_event_addr: tag_value(&event.tags, "a"),
        source_url: tag_value(&event.tags, "r"),
        context: tag_value(&event.tags, "context"),
    })
}

fn tag_value(tags: &[Vec<String>], key: &str) -> Option<String> {
    tags.iter()
        .find(|tag| tag.first().is_some_and(|candidate| candidate == key))
        .and_then(|tag| tag.get(1).cloned())
}

/// Build the kind:9802 tag set. Returns `None` if a hex-shaped field is
/// malformed (defence in depth — `start` already rejects these).
fn highlight_tags(action: &PublishHighlightAction) -> Option<Vec<Vec<String>>> {
    let derived_kinds = external_id::derived_kinds(action.external_ids.iter().map(String::as_str))?;
    let mut tags: Vec<Vec<String>> = Vec::new();
    if let Some(alt) = &action.alt {
        tags.push(vec!["alt".to_string(), alt.clone()]);
    }
    if let Some(event_id) = &action.source_event_id {
        if !is_hex64(event_id) {
            return None;
        }
        tags.push(vec!["e".to_string(), event_id.clone()]);
    }
    if let Some(address) = &action.source_address {
        tags.push(vec!["a".to_string(), address.clone()]);
    }
    if let Some(author) = &action.source_author_pubkey {
        if !is_hex64(author) {
            return None;
        }
        tags.push(vec!["p".to_string(), author.clone()]);
    }
    if let Some(context) = &action.context {
        tags.push(vec!["context".to_string(), context.clone()]);
    }
    for id in &action.external_ids {
        tags.push(vec!["i".to_string(), id.clone()]);
    }
    for kind in derived_kinds {
        tags.push(vec!["k".to_string(), kind]);
    }
    Some(tags)
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
#[path = "action_tests.rs"]
mod tests;
