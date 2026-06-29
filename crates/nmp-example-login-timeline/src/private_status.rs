//! App-private status action for the starter DX proof.
//!
//! The namespace, event kind, schema, Rust payload, and action module all live
//! in this example crate. The generated Swift/Kotlin/TS builders are produced
//! from `action-builders.json`; no built-in NMP action registry row is involved.

#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "private_status/generated/publish_status_generated.rs"]
pub mod publish_status_generated;

use nmp_core::actor::{ActorCommand, PublishCommand};
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRejection,
    ProtocolCommand, ProtocolCommandContext, ProtocolCommandError,
};
use nmp_signer_iface::UnsignedEvent;
use publish_status_generated::app::login_timeline as status_fb;
use serde::{Deserialize, Serialize};

/// App-local namespace declared in `action-builders.json`.
pub const ACTION_NAMESPACE: &str = "app.login_timeline.publish_status";

/// App-private event kind declared in `action-builders.json`.
pub const EVENT_KIND: u32 = 30444;

/// Wire schema version for `schema/publish_status.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

/// A user intent to publish this starter app's private status event.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PublishStatusAction {
    pub title: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topics: Vec<String>,
}

/// App-owned action module for [`ACTION_NAMESPACE`].
pub struct PublishStatusModule;

#[derive(Debug)]
struct PublishStatusCommand {
    action: PublishStatusAction,
    correlation_id: String,
}

impl ActionModule for PublishStatusModule {
    const NAMESPACE: &'static str = ACTION_NAMESPACE;
    type Action = PublishStatusAction;

    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<PublishStatusAction as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        validate_action(&action).map_err(ActionRejection::Invalid)
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(ActorCommand::Protocol(Box::new(PublishStatusCommand {
            action,
            correlation_id: correlation_id.to_string(),
        })));
        Ok(())
    }
}

impl ProtocolCommand for PublishStatusCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let event = status_event(&self.action)
            .map_err(|err| ProtocolCommandError::new(format!("publish_status: {err}")))?;
        ctx.send(ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event,
            correlation_id: Some(self.correlation_id),
            signer_pubkey: None,
        }));
        Ok(())
    }
}

impl ActionPayload for PublishStatusAction {
    const SCHEMA_ID: &'static str = ACTION_NAMESPACE;
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let title = fbb.create_string(&self.title);
        let body = fbb.create_string(&self.body);
        let topics = if self.topics.is_empty() {
            None
        } else {
            let offsets: Vec<_> = self.topics.iter().map(|s| fbb.create_string(s)).collect();
            Some(fbb.create_vector(&offsets))
        };
        let payload = status_fb::PublishStatusPayload::create(
            &mut fbb,
            &status_fb::PublishStatusPayloadArgs {
                schema_version: SCHEMA_VERSION,
                title: Some(title),
                body: Some(body),
                topics,
            },
        );
        status_fb::finish_publish_status_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !status_fb::publish_status_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing APPS file identifier"));
        }
        let root = status_fb::root_as_publish_status_payload(bytes)
            .map_err(|err| malformed(format!("not a valid PublishStatusPayload buffer: {err}")))?;
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        let topics = root
            .topics()
            .map(|v| v.iter().map(str::to_string).collect())
            .unwrap_or_default();
        Ok(PublishStatusAction {
            title: root.title().to_string(),
            body: root.body().to_string(),
            topics,
        })
    }
}

fn status_event(action: &PublishStatusAction) -> Result<UnsignedEvent, String> {
    validate_action(action)?;
    let mut tags = vec![
        vec!["d".to_string(), stable_identifier(&action.title)],
        vec!["alt".to_string(), action.title.trim().to_string()],
    ];
    for topic in &action.topics {
        tags.push(vec!["t".to_string(), topic.trim().to_string()]);
    }
    Ok(UnsignedEvent {
        pubkey: String::new(),
        kind: EVENT_KIND,
        tags,
        content: action.body.trim().to_string(),
        created_at: 0,
    })
}

fn validate_action(action: &PublishStatusAction) -> Result<(), String> {
    if action.title.trim().is_empty() {
        return Err("publish_status requires a non-empty title".to_string());
    }
    if action.body.trim().is_empty() {
        return Err("publish_status requires non-empty body content".to_string());
    }
    if action.topics.iter().any(|topic| topic.trim().is_empty()) {
        return Err("publish_status topics must be non-empty when provided".to_string());
    }
    Ok(())
}

fn stable_identifier(title: &str) -> String {
    title
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

#[cfg(test)]
#[path = "private_status_tests.rs"]
mod tests;
