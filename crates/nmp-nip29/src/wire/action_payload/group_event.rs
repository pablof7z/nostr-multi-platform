//! `ActionPayload` codecs for the neutral host-pinned group-event producers:
//! `share_event_in_group` (kind:11) and `repost_in_group` (kind:16). Both carry
//! a `GroupEventTarget` plus free-form `additional_tags` (ADR-0064 / S9 #1747).

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use crate::action::{GroupEventTarget, RepostInGroupInput, ShareEventInGroupInput};
use crate::group_id::GroupId;

use super::{gate_schema_version, malformed, SCHEMA_VERSION};

use super::repost_in_group_action_generated::nmp::nip_29 as repost_fb;
use super::share_event_in_group_action_generated::nmp::nip_29 as share_fb;

// `share` and `repost` are structurally identical (distinct schema/identifier),
// so the codec bodies differ only in the generated module. A macro keeps the two
// impls in lockstep without copy-paste drift.
macro_rules! group_event_payload {
    (
        $input:ty,
        $schema_id:literal,
        $fb:ident,
        $payload:ident,
        $payload_args:ident,
        $finish:ident,
        $root_as:ident,
        $has_identifier:ident,
        $identifier:literal
    ) => {
        impl ActionPayload for $input {
            const SCHEMA_ID: &'static str = $schema_id;
            const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

            fn encode(&self) -> Vec<u8> {
                let mut fbb = flatbuffers::FlatBufferBuilder::new();
                let host_relay_url = fbb.create_string(&self.group.host_relay_url);
                let local_id = fbb.create_string(&self.group.local_id);
                let group = $fb::GroupRef::create(
                    &mut fbb,
                    &$fb::GroupRefArgs {
                        host_relay_url: Some(host_relay_url),
                        local_id: Some(local_id),
                    },
                );
                let event_id = fbb.create_string(&self.target.event_id);
                let author_pubkey = self
                    .target
                    .author_pubkey
                    .as_ref()
                    .map(|s| fbb.create_string(s));
                let target = $fb::GroupEventTarget::create(
                    &mut fbb,
                    &$fb::GroupEventTargetArgs {
                        event_id: Some(event_id),
                        author_pubkey,
                    },
                );
                let content = fbb.create_string(&self.content);
                let tag_offsets: Vec<_> = self
                    .additional_tags
                    .iter()
                    .map(|tag| {
                        let value_offsets: Vec<_> =
                            tag.iter().map(|v| fbb.create_string(v)).collect();
                        let values = fbb.create_vector(&value_offsets);
                        $fb::StringTag::create(
                            &mut fbb,
                            &$fb::StringTagArgs {
                                values: Some(values),
                            },
                        )
                    })
                    .collect();
                let additional_tags = fbb.create_vector(&tag_offsets);
                let payload = $fb::$payload::create(
                    &mut fbb,
                    &$fb::$payload_args {
                        schema_version: SCHEMA_VERSION,
                        group: Some(group),
                        target: Some(target),
                        content: Some(content),
                        additional_tags: Some(additional_tags),
                    },
                );
                $fb::$finish(&mut fbb, payload);
                fbb.finished_data().to_vec()
            }

            fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
                if bytes.len() < 8 || !$fb::$has_identifier(bytes) {
                    return Err(malformed(concat!(
                        "missing ",
                        $identifier,
                        " file identifier"
                    )));
                }
                let root = $fb::$root_as(bytes).map_err(|e| {
                    malformed(format!(
                        concat!("not a valid ", stringify!($payload), " buffer: {}"),
                        e
                    ))
                })?;
                gate_schema_version(root.schema_version())?;
                let group = root.group();
                let target = root.target();
                let additional_tags = root
                    .additional_tags()
                    .map(|tags| {
                        tags.iter()
                            .map(|tag| {
                                tag.values()
                                    .map(|vs| vs.iter().map(str::to_string).collect())
                                    .unwrap_or_default()
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(<$input>::from_decoded_parts(
                    GroupId::new(group.host_relay_url(), group.local_id()),
                    GroupEventTarget {
                        event_id: target.event_id().to_string(),
                        author_pubkey: target.author_pubkey().map(str::to_string),
                    },
                    root.content().unwrap_or_default().to_string(),
                    additional_tags,
                ))
            }
        }
    };
}

// A tiny shared constructor so the macro body stays type-agnostic over the two
// structurally identical inputs.
impl ShareEventInGroupInput {
    fn from_decoded_parts(
        group: GroupId,
        target: GroupEventTarget,
        content: String,
        additional_tags: Vec<Vec<String>>,
    ) -> Self {
        Self {
            group,
            target,
            content,
            additional_tags,
        }
    }
}

impl RepostInGroupInput {
    fn from_decoded_parts(
        group: GroupId,
        target: GroupEventTarget,
        content: String,
        additional_tags: Vec<Vec<String>>,
    ) -> Self {
        Self {
            group,
            target,
            content,
            additional_tags,
        }
    }
}

group_event_payload!(
    ShareEventInGroupInput,
    "nmp.nip29.share_event_in_group",
    share_fb,
    ShareEventInGroupPayload,
    ShareEventInGroupPayloadArgs,
    finish_share_event_in_group_payload_buffer,
    root_as_share_event_in_group_payload,
    share_event_in_group_payload_buffer_has_identifier,
    "N29S"
);

group_event_payload!(
    RepostInGroupInput,
    "nmp.nip29.repost_in_group",
    repost_fb,
    RepostInGroupPayload,
    RepostInGroupPayloadArgs,
    finish_repost_in_group_payload_buffer,
    root_as_repost_in_group_payload,
    repost_in_group_payload_buffer_has_identifier,
    "N29O"
);
