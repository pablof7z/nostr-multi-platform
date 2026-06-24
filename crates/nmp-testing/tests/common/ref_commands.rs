use nmp_core::actor::RefsCommand;
use nmp_core::nip21::{parse_nostr_uri, NostrUri};
use nmp_core::{EventShape, RefLiveness, RefNamespace, RefShape};

fn event_key_and_hints_from_uri(uri: &str) -> Option<(String, Vec<String>)> {
    match parse_nostr_uri(uri).ok()? {
        NostrUri::Event {
            event_id, relays, ..
        } => Some((event_id, relays)),
        NostrUri::Address {
            identifier,
            pubkey,
            kind,
            relays,
        } => Some((format!("{kind}:{pubkey}:{identifier}"), relays)),
        NostrUri::Profile { .. } => None,
    }
}

pub fn resolve_event_embed(uri: &str, consumer_id: &str) -> RefsCommand {
    let (key, hints) = event_key_and_hints_from_uri(uri)
        .unwrap_or_else(|| panic!("test fixture must be an event/address URI: {uri}"));
    RefsCommand::Resolve {
        namespace: RefNamespace::Event,
        key,
        consumer_id: consumer_id.to_string(),
        shape: RefShape::Event(EventShape::Embed),
        liveness: RefLiveness::CacheOk,
        force: false,
        hints,
    }
}

pub fn release_event_ref(uri: &str, consumer_id: &str) -> RefsCommand {
    let (key, _) = event_key_and_hints_from_uri(uri)
        .unwrap_or_else(|| panic!("test fixture must be an event/address URI: {uri}"));
    RefsCommand::Release {
        namespace: RefNamespace::Event,
        key,
        consumer_id: consumer_id.to_string(),
    }
}
