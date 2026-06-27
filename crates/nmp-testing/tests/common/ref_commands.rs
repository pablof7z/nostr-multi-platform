use nmp_core::actor::RefsCommand;
use nmp_core::nip21::{parse_nostr_uri, NostrUri};
use nmp_core::{EventShape, RefLiveness, RefNamespace, RefResolveMetadata, RefShape};

fn event_ref_from_uri(uri: &str) -> Option<(String, RefResolveMetadata)> {
    match parse_nostr_uri(uri).ok()? {
        NostrUri::Event {
            event_id,
            relays,
            author,
            ..
        } => Some((
            event_id,
            RefResolveMetadata {
                hints: relays,
                event_author: author,
            },
        )),
        NostrUri::Address {
            identifier,
            pubkey,
            kind,
            relays,
        } => Some((
            format!("{kind}:{pubkey}:{identifier}"),
            RefResolveMetadata {
                hints: relays,
                event_author: None,
            },
        )),
        NostrUri::Profile { .. } => None,
    }
}

pub fn resolve_event_embed(uri: &str, consumer_id: &str) -> RefsCommand {
    let (key, metadata) = event_ref_from_uri(uri)
        .unwrap_or_else(|| panic!("test fixture must be an event/address URI: {uri}"));
    RefsCommand::ResolveWithMetadata {
        namespace: RefNamespace::Event,
        key,
        consumer_id: consumer_id.to_string(),
        shape: RefShape::Event(EventShape::Embed),
        liveness: RefLiveness::CacheOk,
        force: false,
        metadata,
    }
}

pub fn release_event_ref(uri: &str, consumer_id: &str) -> RefsCommand {
    let (key, _) = event_ref_from_uri(uri)
        .unwrap_or_else(|| panic!("test fixture must be an event/address URI: {uri}"));
    RefsCommand::Release {
        namespace: RefNamespace::Event,
        key,
        consumer_id: consumer_id.to_string(),
    }
}
