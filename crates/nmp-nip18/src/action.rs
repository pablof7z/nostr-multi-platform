//! `nmp.nip18.repost` — the NIP-18 repost publish [`ActionModule`]
//! (M14-1 / PR2 #2145).
//!
//! Rust owns the kind:6 repost construction: the host passes the target event id
//! + author pubkey, and this module builds the kind:6 event with `["e",
//! event_id]` + `["p", author_pubkey]` tags and an empty content — the
//! byte-for-byte twin of the tag output the retired `ChirpActionIntent::Repost`
//! spec produced. The shell never assembles a tag.
//!
//! `pubkey` and `created_at` are D7 sentinels (`""` / `0`); the actor re-stamps
//! both from the active `Keys` + the wall clock before signing.

use nmp_core::actor::{ActorCommand, PublishCommand};
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRegistrar,
    ActionRejection, ProtocolDescriptor,
};
use nmp_core::tags::{e_tag, p_tag};
use nmp_signer_iface::UnsignedEvent;
use serde::{Deserialize, Serialize};

use crate::KIND_REPOST;

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
#[path = "wire/generated/repost_generated.rs"]
mod repost_generated;

use repost_generated::nmp::nip_18 as repost_fb;

/// Wire schema version for the nip18 repost payload. Bump on any breaking change
/// to `repost.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

/// Wire shape for `nmp.nip18.repost` — the target event id + its author pubkey
/// (Rust owns the kind:6 + `e`/`p` tag construction).
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RepostInput {
    /// Target event id (hex) being reposted (the kind:6 `e`-tag).
    pub event_id: String,
    /// Target event author pubkey (hex) (the kind:6 `p`-tag).
    pub author_pubkey: String,
}

impl ActionPayload for RepostInput {
    const SCHEMA_ID: &'static str = "nmp.nip18.repost";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let event_id = fbb.create_string(&self.event_id);
        let author_pubkey = fbb.create_string(&self.author_pubkey);
        let payload = repost_fb::RepostPayload::create(
            &mut fbb,
            &repost_fb::RepostPayloadArgs {
                schema_version: SCHEMA_VERSION,
                event_id: Some(event_id),
                author_pubkey: Some(author_pubkey),
            },
        );
        repost_fb::finish_repost_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !repost_fb::repost_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N18R file identifier"));
        }
        let root = repost_fb::root_as_repost_payload(bytes)
            .map_err(|e| malformed(format!("not a valid RepostPayload buffer: {e}")))?;
        // Gate FIRST.
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(RepostInput {
            event_id: root.event_id().to_string(),
            author_pubkey: root.author_pubkey().to_string(),
        })
    }
}

/// The `nmp.nip18.repost` [`ActionModule`] — validates the target, builds the
/// unsigned kind:6 repost, and dispatches it through the standard publish engine.
pub struct RepostModule;

impl RepostModule {
    /// Build the unsigned kind:6 repost: `["e", event_id]` + `["p",
    /// author_pubkey]`, empty content. `pubkey`/`created_at` are D7 sentinels.
    fn build_unsigned(action: &RepostInput) -> UnsignedEvent {
        UnsignedEvent {
            pubkey: String::new(),
            kind: KIND_REPOST,
            tags: vec![
                e_tag(&action.event_id, None, None),
                p_tag(&action.author_pubkey, None),
            ],
            content: String::new(),
            created_at: 0,
        }
    }
}

impl ActionModule for RepostModule {
    const NAMESPACE: &'static str = "nmp.nip18.repost";
    type Action = RepostInput;

    /// Opt into the typed FlatBuffers payload doorway; the fail-closed
    /// `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<RepostInput as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        if !is_hex64(&action.event_id) {
            return Err(ActionRejection::Invalid(
                "repost requires a 64-hex event_id".to_string(),
            ));
        }
        if !is_hex64(&action.author_pubkey) {
            return Err(ActionRejection::Invalid(
                "repost requires a 64-hex author_pubkey".to_string(),
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
        send(ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event: Self::build_unsigned(&action),
            correlation_id: Some(correlation_id.to_string()),
            signer_pubkey: None,
        }));
        Ok(())
    }
}

/// Typed protocol descriptor for the NIP-18 repost action.
///
/// Registered as a **yielding default** (ADR-0049 Part 1): an app that
/// pre-registers its own `nmp.nip18.repost` handler pre-empts this one
/// regardless of call order.
pub struct Nip18Descriptor;

impl ProtocolDescriptor for Nip18Descriptor {
    fn register_actions(&self, app: &mut impl ActionRegistrar) {
        app.register_default_action(RepostModule);
    }
}

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
#[path = "action_tests.rs"]
mod tests;
