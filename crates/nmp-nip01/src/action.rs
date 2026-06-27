//! `nmp.nip01.publish_note` — the NIP-01 short-text-note publish [`ActionModule`]
//! (M14-1 / PR2 #2145).
//!
//! Rust owns ALL NIP-10 reply-tag construction. The host passes raw note content
//! plus, when the note is a reply, the parent's protocol fields; this module
//! reconstructs the parent [`NoteRecord`] and builds the marked-form root/reply
//! `e`-tags + thread `p`-tags through [`crate::Note`] — the byte-for-byte twin of
//! the tag output the retired `ChirpActionIntent::PublishNote` spec produced. The
//! shell never assembles a tag.
//!
//! # Reply-field semantics (keyed on `reply_event_id`)
//!
//! * absent → a root note (`Note::new(content).build("", 0)`, no tags).
//! * present, `reply_author_pubkey` absent → the minimal root+reply `e`-tags on
//!   the same id (the legacy `reply_to_event_id`-only behaviour).
//! * present + `reply_author_pubkey` → reconstruct the parent `NoteRecord` and
//!   build the full NIP-10 reply via `Note::reply_to(parent)`.
//!
//! `pubkey` and `created_at` are D7 sentinels (`""` / `0`); the actor re-stamps
//! both from the active `Keys` + the wall clock before signing — exactly as the
//! NIP-25 reaction publish does.

use nmp_core::actor::{ActorCommand, PublishCommand};
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRegistrar,
    ActionRejection, ProtocolDescriptor,
};
use nmp_core::tags::{e_tag, EventRef, Nip10Refs};
use nmp_signer_iface::UnsignedEvent;
use serde::{Deserialize, Serialize};

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
#[path = "wire/generated/publish_note_generated.rs"]
mod publish_note_generated;

use publish_note_generated::nmp::nip_01 as note_fb;

use crate::decode::NoteRecord;
use crate::kinds::KIND_SHORT_TEXT_NOTE;
use crate::Note;

/// Wire schema version for the nip01 publish-note payload. Bump on any breaking
/// change to `publish_note.fbs`.
pub const SCHEMA_VERSION: u32 = 2;

/// Wire shape for `nmp.nip01.publish_note` — raw note content plus the parent's
/// protocol fields when this note is a reply (Rust owns all tag construction).
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PublishNoteInput {
    /// The note body — becomes the kind:1 `content`.
    pub content: String,
    /// Parent event id (hex) when replying; `None`/empty → a root note.
    #[serde(default)]
    pub reply_event_id: Option<String>,
    /// Parent author pubkey (hex). With `reply_event_id`, drives the full
    /// NIP-10 reply (root/reply markers + `p`-tags).
    #[serde(default)]
    pub reply_author_pubkey: Option<String>,
    /// Thread root event id (hex), when the parent already pointed at a root.
    #[serde(default)]
    pub reply_root_event_id: Option<String>,
    /// Optional relay hint for the thread root `e`-tag.
    #[serde(default)]
    pub reply_root_relay: Option<String>,
    /// Parent's mentioned pubkeys, re-notified as thread `p`-tags (deduplicated).
    #[serde(default)]
    pub reply_mentioned_pubkeys: Vec<String>,
}

impl PublishNoteInput {
    /// Build the unsigned kind:1 note this input describes, owning every NIP-10
    /// tag decision in Rust. `pubkey`/`created_at` are D7 sentinels re-stamped by
    /// the actor before signing.
    fn build_unsigned(&self) -> Result<UnsignedEvent, String> {
        let reply_id = self
            .reply_event_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty());
        match (reply_id, self.reply_author_pubkey.as_deref()) {
            // Full NIP-10 reply: reconstruct the parent and delegate to `Note`.
            (Some(id), Some(author)) if !author.trim().is_empty() => {
                let parent = self.parent_note_record(id, author);
                Note::new(self.content.clone())
                    .reply_to(&parent)
                    .build("", 0)
                    .map_err(|e| e.to_string())
            }
            // Minimal root+reply `e`-tags on the same id (no author known).
            (Some(id), _) => {
                // Validate non-empty content the same way `Note::build` does.
                Note::new(self.content.clone())
                    .build("", 0)
                    .map_err(|e| e.to_string())?;
                Ok(UnsignedEvent {
                    pubkey: String::new(),
                    kind: KIND_SHORT_TEXT_NOTE,
                    tags: vec![
                        e_tag(id, None, Some("root")),
                        e_tag(id, None, Some("reply")),
                    ],
                    content: self.content.clone(),
                    created_at: 0,
                })
            }
            // Root note, no tags.
            (None, _) => Note::new(self.content.clone())
                .build("", 0)
                .map_err(|e| e.to_string()),
        }
    }

    /// Reconstruct the parent [`NoteRecord`] from the flat reply fields — the
    /// inverse of the retired `ReplyTargetInput::into_note_record`.
    fn parent_note_record(&self, event_id: &str, author: &str) -> NoteRecord {
        let refs = Nip10Refs {
            root: self.reply_root_event_id.clone().map(|id| EventRef {
                id,
                relay: self.reply_root_relay.clone(),
                marker: Some("root".to_string()),
            }),
            reply: None,
            mentions: Vec::new(),
            mentioned_pubkeys: self.reply_mentioned_pubkeys.clone(),
        };
        NoteRecord {
            event_id: event_id.to_string(),
            author: author.to_string(),
            created_at: 0,
            content: String::new(),
            refs,
        }
    }
}

impl ActionPayload for PublishNoteInput {
    const SCHEMA_ID: &'static str = "nmp.nip01.publish_note";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let content = fbb.create_string(&self.content);
        let reply_event_id = self.reply_event_id.as_ref().map(|s| fbb.create_string(s));
        let reply_author_pubkey = self
            .reply_author_pubkey
            .as_ref()
            .map(|s| fbb.create_string(s));
        let reply_root_event_id = self
            .reply_root_event_id
            .as_ref()
            .map(|s| fbb.create_string(s));
        let reply_root_relay = self.reply_root_relay.as_ref().map(|s| fbb.create_string(s));
        let reply_mentioned_pubkeys = if self.reply_mentioned_pubkeys.is_empty() {
            None
        } else {
            let offsets: Vec<_> = self
                .reply_mentioned_pubkeys
                .iter()
                .map(|s| fbb.create_string(s))
                .collect();
            Some(fbb.create_vector(&offsets))
        };
        let payload = note_fb::PublishNotePayload::create(
            &mut fbb,
            &note_fb::PublishNotePayloadArgs {
                schema_version: SCHEMA_VERSION,
                content: Some(content),
                reply_event_id,
                reply_author_pubkey,
                reply_root_event_id,
                reply_root_relay,
                reply_mentioned_pubkeys,
            },
        );
        note_fb::finish_publish_note_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !note_fb::publish_note_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N01N file identifier"));
        }
        let root = note_fb::root_as_publish_note_payload(bytes)
            .map_err(|e| malformed(format!("not a valid PublishNotePayload buffer: {e}")))?;
        // Gate FIRST.
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(PublishNoteInput {
            content: root.content().to_string(),
            reply_event_id: root.reply_event_id().map(str::to_string),
            reply_author_pubkey: root.reply_author_pubkey().map(str::to_string),
            reply_root_event_id: root.reply_root_event_id().map(str::to_string),
            reply_root_relay: root.reply_root_relay().map(str::to_string),
            reply_mentioned_pubkeys: root
                .reply_mentioned_pubkeys()
                .map(|v| v.iter().map(str::to_string).collect())
                .unwrap_or_default(),
        })
    }
}

/// The `nmp.nip01.publish_note` [`ActionModule`] — validates the note, then
/// builds the unsigned kind:1 (with all NIP-10 tag construction) and dispatches
/// it through the standard publish engine.
pub struct PublishNoteModule;

impl ActionModule for PublishNoteModule {
    const NAMESPACE: &'static str = "nmp.nip01.publish_note";
    type Action = PublishNoteInput;

    /// Opt into the typed FlatBuffers payload doorway; the fail-closed
    /// `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<PublishNoteInput as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        if action.content.trim().is_empty() {
            return Err(ActionRejection::Invalid(
                "publish_note requires non-empty content".to_string(),
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
        let event = action.build_unsigned()?;
        send(ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event,
            correlation_id: Some(correlation_id.to_string()),
            signer_pubkey: None,
        }));
        Ok(())
    }
}

/// Typed protocol descriptor for the NIP-01 publish-note action.
///
/// Registered as a **yielding default** (ADR-0049 Part 1): an app that
/// pre-registers its own `nmp.nip01.publish_note` handler pre-empts this one
/// regardless of call order.
pub struct Nip01Descriptor;

impl ProtocolDescriptor for Nip01Descriptor {
    fn register_actions(&self, app: &mut impl ActionRegistrar) {
        app.register_default_action(PublishNoteModule);
    }
}

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

#[cfg(test)]
#[path = "action_tests.rs"]
mod tests;
