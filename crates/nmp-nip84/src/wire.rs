//! Typed FlatBuffers payload codec for `nmp.nip84.publish_highlight`.

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
#[path = "wire/generated/highlight_generated.rs"]
pub mod generated;

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use generated::nmp::nip_84 as fb;

use crate::action::{HighlightAttribution, HighlightSource, PublishHighlightInput};

pub const SCHEMA_VERSION: u32 = 1;

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

impl ActionPayload for PublishHighlightInput {
    const SCHEMA_ID: &'static str = "nmp.nip84.publish_highlight";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let sources = encode_sources(&mut fbb, &self.source_refs);
        let attributions = encode_attributions(&mut fbb, &self.attributions);
        let highlighted_text = fbb.create_string(&self.highlighted_text);
        let context = self.context.as_ref().map(|s| fbb.create_string(s));
        let comment = self.comment.as_ref().map(|s| fbb.create_string(s));
        let payload = fb::PublishHighlightPayload::create(
            &mut fbb,
            &fb::PublishHighlightPayloadArgs {
                schema_version: SCHEMA_VERSION,
                highlighted_text: Some(highlighted_text),
                context,
                comment,
                source_refs: sources,
                attributions,
            },
        );
        fb::finish_publish_highlight_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !fb::publish_highlight_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N84H file identifier"));
        }
        let root = fb::root_as_publish_highlight_payload(bytes)
            .map_err(|e| malformed(format!("not a valid PublishHighlightPayload buffer: {e}")))?;
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(PublishHighlightInput {
            highlighted_text: root.highlighted_text().to_string(),
            context: root.context().map(str::to_string),
            comment: root.comment().map(str::to_string),
            source_refs: decode_sources(root.source_refs())?,
            attributions: decode_attributions(root.attributions()),
        })
    }
}

type FbSourceVector<'a> =
    flatbuffers::Vector<'a, flatbuffers::ForwardsUOffset<fb::HighlightSource<'a>>>;
type FbAttributionVector<'a> =
    flatbuffers::Vector<'a, flatbuffers::ForwardsUOffset<fb::HighlightAttribution<'a>>>;

fn encode_sources<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    sources: &[HighlightSource],
) -> Option<flatbuffers::WIPOffset<FbSourceVector<'a>>> {
    if sources.is_empty() {
        return None;
    }
    let offsets: Vec<_> = sources
        .iter()
        .map(|source| {
            let wire = source_wire_fields(source);
            let value = fbb.create_string(wire.value);
            let relay = wire.relay.map(|s| fbb.create_string(s));
            let external_kind = wire.external_kind.map(|s| fbb.create_string(s));
            let hint_url = wire.hint_url.map(|s| fbb.create_string(s));
            fb::HighlightSource::create(
                fbb,
                &fb::HighlightSourceArgs {
                    kind: wire.kind,
                    value: Some(value),
                    relay,
                    external_kind,
                    hint_url,
                },
            )
        })
        .collect();
    Some(fbb.create_vector(&offsets))
}

struct SourceWireFields<'a> {
    kind: fb::HighlightSourceKind,
    value: &'a str,
    relay: Option<&'a str>,
    external_kind: Option<&'a str>,
    hint_url: Option<&'a str>,
}

fn source_wire_fields(source: &HighlightSource) -> SourceWireFields<'_> {
    match source {
        HighlightSource::Event { event_id, relay } => SourceWireFields {
            kind: fb::HighlightSourceKind::Event,
            value: event_id,
            relay: relay.as_deref(),
            external_kind: None,
            hint_url: None,
        },
        HighlightSource::Address { coordinate, relay } => SourceWireFields {
            kind: fb::HighlightSourceKind::Address,
            value: coordinate,
            relay: relay.as_deref(),
            external_kind: None,
            hint_url: None,
        },
        HighlightSource::Url { url } => SourceWireFields {
            kind: fb::HighlightSourceKind::Url,
            value: url,
            relay: None,
            external_kind: None,
            hint_url: None,
        },
        HighlightSource::External {
            external_id,
            external_kind,
            hint_url,
        } => SourceWireFields {
            kind: fb::HighlightSourceKind::External,
            value: external_id,
            relay: None,
            external_kind: Some(external_kind),
            hint_url: hint_url.as_deref(),
        },
    }
}

fn decode_sources(
    sources: Option<FbSourceVector<'_>>,
) -> Result<Vec<HighlightSource>, ActionPayloadDecodeError> {
    let Some(sources) = sources else {
        return Ok(Vec::new());
    };
    sources
        .iter()
        .map(|source| match source.kind() {
            fb::HighlightSourceKind::Event => Ok(HighlightSource::Event {
                event_id: source.value().to_string(),
                relay: source.relay().map(str::to_string),
            }),
            fb::HighlightSourceKind::Address => Ok(HighlightSource::Address {
                coordinate: source.value().to_string(),
                relay: source.relay().map(str::to_string),
            }),
            fb::HighlightSourceKind::Url => Ok(HighlightSource::Url {
                url: source.value().to_string(),
            }),
            fb::HighlightSourceKind::External => Ok(HighlightSource::External {
                external_id: source.value().to_string(),
                external_kind: source.external_kind().unwrap_or_default().to_string(),
                hint_url: source.hint_url().map(str::to_string),
            }),
            other => Err(malformed(format!(
                "unknown HighlightSourceKind discriminant {}",
                other.0
            ))),
        })
        .collect()
}

fn encode_attributions<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    attributions: &[HighlightAttribution],
) -> Option<flatbuffers::WIPOffset<FbAttributionVector<'a>>> {
    if attributions.is_empty() {
        return None;
    }
    let offsets: Vec<_> = attributions
        .iter()
        .map(|attribution| {
            let pubkey = fbb.create_string(&attribution.pubkey);
            let relay = attribution.relay.as_ref().map(|s| fbb.create_string(s));
            let role = attribution.role.as_ref().map(|s| fbb.create_string(s));
            fb::HighlightAttribution::create(
                fbb,
                &fb::HighlightAttributionArgs {
                    pubkey: Some(pubkey),
                    relay,
                    role,
                },
            )
        })
        .collect();
    Some(fbb.create_vector(&offsets))
}

fn decode_attributions(attributions: Option<FbAttributionVector<'_>>) -> Vec<HighlightAttribution> {
    attributions
        .map(|attributions| {
            attributions
                .iter()
                .map(|attribution| HighlightAttribution {
                    pubkey: attribution.pubkey().to_string(),
                    relay: attribution.relay().map(str::to_string),
                    role: attribution.role().map(str::to_string),
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "wire/tests.rs"]
mod tests;
