use std::collections::BTreeSet;

use nmp_core::actor::{ActorCommand, PublishCommand};
use nmp_core::canonical_relay_url;
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRegistrar,
    ActionRejection, ProtocolCommand, ProtocolCommandContext, ProtocolCommandError,
    ProtocolDescriptor,
};
use nmp_kinds::KIND_HIGHLIGHT;
use nmp_signer_iface::UnsignedEvent;
use serde::{Deserialize, Serialize};

pub const PUBLISH_HIGHLIGHT_NAMESPACE: &str = "nmp.nip84.publish_highlight";

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PublishHighlightInput {
    #[serde(default)]
    pub highlighted_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_refs: Vec<HighlightSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attributions: Vec<HighlightAttribution>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HighlightSource {
    /// NIP-84 `e` source tag.
    Event {
        event_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        relay: Option<String>,
    },
    /// NIP-84 `a` source tag.
    Address {
        coordinate: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        relay: Option<String>,
    },
    /// NIP-84 `r` source URL tag.
    Url { url: String },
    /// NIP-73 `i` source tag plus matching `k` tag.
    External {
        external_id: String,
        external_kind: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hint_url: Option<String>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HighlightAttribution {
    pub pubkey: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Debug)]
pub struct PublishHighlightCommand {
    action: PublishHighlightInput,
    correlation_id: String,
}

pub struct PublishHighlightModule;

impl ActionModule for PublishHighlightModule {
    const NAMESPACE: &'static str = PUBLISH_HIGHLIGHT_NAMESPACE;
    type Action = PublishHighlightInput;

    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<PublishHighlightInput as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        validate_highlight(&action).map_err(ActionRejection::Invalid)
    }

    fn execute(
        &self,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        validate_highlight(&action)?;
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
        let event = build_highlight_event(&self.action).map_err(ProtocolCommandError::new)?;
        ctx.send(ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event,
            correlation_id: Some(self.correlation_id),
            signer_pubkey: None,
        }));
        Ok(())
    }
}

pub struct Nip84Descriptor;

impl ProtocolDescriptor for Nip84Descriptor {
    fn register_actions(&self, app: &mut impl ActionRegistrar) {
        app.register_default_action(PublishHighlightModule);
    }
}

pub fn build_highlight_event(input: &PublishHighlightInput) -> Result<UnsignedEvent, String> {
    validate_highlight(input)?;
    let mut tags = Vec::new();
    let mut external_kinds = BTreeSet::new();
    for source in &input.source_refs {
        append_source_tags(source, &mut tags, &mut external_kinds)?;
    }
    if let Some(context) = nonempty_trimmed(input.context.as_deref()) {
        tags.push(vec!["context".to_string(), context.to_string()]);
    }
    if let Some(comment) = nonempty_trimmed(input.comment.as_deref()) {
        tags.push(vec!["comment".to_string(), comment.to_string()]);
    }
    for attribution in &input.attributions {
        tags.push(attribution_tag(attribution)?);
    }
    Ok(UnsignedEvent {
        pubkey: String::new(),
        kind: KIND_HIGHLIGHT,
        tags,
        content: input.highlighted_text.clone(),
        created_at: 0,
    })
}

fn validate_highlight(input: &PublishHighlightInput) -> Result<(), String> {
    if input.highlighted_text.trim().is_empty() && input.source_refs.is_empty() {
        return Err(
            "highlight publish requires highlighted_text or at least one source_ref".to_string(),
        );
    }
    for source in &input.source_refs {
        validate_source(source)?;
    }
    for attribution in &input.attributions {
        validate_attribution(attribution)?;
    }
    Ok(())
}

fn validate_source(source: &HighlightSource) -> Result<(), String> {
    match source {
        HighlightSource::Event { event_id, relay } => {
            if !is_hex64(event_id) {
                return Err("highlight event source requires a 64-hex event_id".to_string());
            }
            normalize_optional_relay(relay, "highlight event source relay")?;
        }
        HighlightSource::Address { coordinate, relay } => {
            if !valid_address_coordinate(coordinate) {
                return Err(
                    "highlight address source requires a kind:pubkey:d coordinate".to_string(),
                );
            }
            normalize_optional_relay(relay, "highlight address source relay")?;
        }
        HighlightSource::Url { url } => {
            if !valid_http_url(url) {
                return Err("highlight URL source requires an http:// or https:// URL".to_string());
            }
        }
        HighlightSource::External {
            external_id,
            external_kind,
            hint_url,
        } => {
            if !valid_external_token(external_id) {
                return Err("highlight external source requires a valid NIP-73 i tag".to_string());
            }
            if !valid_external_token(external_kind) {
                return Err("highlight external source requires a valid NIP-73 k tag".to_string());
            }
            if hint_url.as_deref().is_some_and(|url| !valid_http_url(url)) {
                return Err("highlight external source hint_url must be http(s)".to_string());
            }
        }
    }
    Ok(())
}

fn append_source_tags(
    source: &HighlightSource,
    tags: &mut Vec<Vec<String>>,
    external_kinds: &mut BTreeSet<String>,
) -> Result<(), String> {
    match source {
        HighlightSource::Event { event_id, relay } => tags.push(tag_with_optional_relay(
            "e",
            &event_id.to_ascii_lowercase(),
            normalize_optional_relay(relay, "highlight event source relay")?,
        )),
        HighlightSource::Address { coordinate, relay } => tags.push(tag_with_optional_relay(
            "a",
            &normalize_address_coordinate(coordinate),
            normalize_optional_relay(relay, "highlight address source relay")?,
        )),
        HighlightSource::Url { url } => {
            tags.push(vec![
                "r".to_string(),
                url.trim().to_string(),
                "source".to_string(),
            ]);
        }
        HighlightSource::External {
            external_id,
            external_kind,
            hint_url,
        } => {
            let mut tag = vec!["i".to_string(), external_id.trim().to_string()];
            if let Some(hint) = nonempty_trimmed(hint_url.as_deref()) {
                tag.push(hint.to_string());
            }
            tags.push(tag);
            let kind = external_kind.trim().to_string();
            if external_kinds.insert(kind.clone()) {
                tags.push(vec!["k".to_string(), kind]);
            }
        }
    }
    Ok(())
}

fn validate_attribution(attribution: &HighlightAttribution) -> Result<(), String> {
    if !is_hex64(&attribution.pubkey) {
        return Err("highlight attribution pubkey must be 64-hex".to_string());
    }
    normalize_optional_relay(&attribution.relay, "highlight attribution relay")?;
    if attribution
        .role
        .as_deref()
        .is_some_and(|role| !valid_external_token(role))
    {
        return Err("highlight attribution role must be non-empty and log-safe".to_string());
    }
    Ok(())
}

fn attribution_tag(attribution: &HighlightAttribution) -> Result<Vec<String>, String> {
    let relay = normalize_optional_relay(&attribution.relay, "highlight attribution relay")?;
    let role = nonempty_trimmed(attribution.role.as_deref());
    let mut tag = vec!["p".to_string(), attribution.pubkey.to_ascii_lowercase()];
    if let Some(relay) = relay {
        tag.push(relay);
    } else if role.is_some() {
        tag.push(String::new());
    }
    if let Some(role) = role {
        tag.push(role.to_string());
    }
    Ok(tag)
}

fn tag_with_optional_relay(kind: &str, value: &str, relay: Option<String>) -> Vec<String> {
    let mut tag = vec![kind.to_string(), value.to_string()];
    if let Some(relay) = relay {
        tag.push(relay);
    }
    tag
}

fn normalize_optional_relay(relay: &Option<String>, label: &str) -> Result<Option<String>, String> {
    match nonempty_trimmed(relay.as_deref()) {
        Some(raw) => canonical_relay_url(raw)
            .map(Some)
            .ok_or_else(|| format!("{label} must be a ws:// or wss:// URL")),
        None => Ok(None),
    }
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn valid_address_coordinate(value: &str) -> bool {
    let mut parts = value.splitn(3, ':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(kind), Some(pubkey), Some(identifier)) => {
            kind.parse::<u32>().is_ok() && is_hex64(pubkey) && !identifier.is_empty()
        }
        _ => false,
    }
}

fn normalize_address_coordinate(value: &str) -> String {
    let mut parts = value.splitn(3, ':');
    let kind = parts.next().unwrap_or_default();
    let pubkey = parts.next().unwrap_or_default().to_ascii_lowercase();
    let identifier = parts.next().unwrap_or_default();
    format!("{kind}:{pubkey}:{identifier}")
}

fn valid_http_url(value: &str) -> bool {
    let trimmed = value.trim();
    (trimmed.starts_with("https://") || trimmed.starts_with("http://"))
        && !trimmed.bytes().any(|b| b.is_ascii_control() || b == b' ')
}

fn valid_external_token(value: &str) -> bool {
    nonempty_trimmed(Some(value))
        .is_some_and(|trimmed| !trimmed.bytes().any(|b| b.is_ascii_control() || b == b' '))
}

fn nonempty_trimmed(value: Option<&str>) -> Option<&str> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

#[cfg(test)]
#[path = "action_tests.rs"]
mod tests;
