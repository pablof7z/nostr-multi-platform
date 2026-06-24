use crate::kernel::{AppRelay, Kernel};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

const WRITE_RELAY: &str = "wss://relay.test";

fn signed_profile(keys: &nostr::Keys) -> SignedEvent {
    let event = nostr::EventBuilder::new(nostr::Kind::from(0u16), r#"{"display_name":"Alice"}"#)
        .custom_created_at(nostr::Timestamp::from_secs(1_700_000_000))
        .sign_with_keys(keys)
        .expect("generated keys sign");
    SignedEvent {
        id: event.id.to_hex(),
        sig: event.sig.to_string(),
        unsigned: UnsignedEvent {
            pubkey: event.pubkey.to_hex(),
            kind: event.kind.as_u16() as u32,
            tags: event
                .tags
                .iter()
                .map(|tag: &nostr::Tag| tag.as_slice().to_vec())
                .collect(),
            content: event.content.clone(),
            created_at: event.created_at.as_secs(),
        },
    }
}

#[test]
fn active_account_local_write_relay_routes_profile_before_kind10002() {
    let keys = nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_configured_relays(vec![AppRelay::new(
        WRITE_RELAY.to_string(),
        "both".to_string(),
    )]);
    kernel.set_active_account(author.clone());

    let outbound = kernel.run_publish_engine_at(
        &signed_profile(&keys),
        &[],
        crate::publish::PublishTarget::Auto,
        None,
        1_000,
    );

    assert_eq!(outbound.len(), 1, "active account fallback must route");
    assert_eq!(outbound[0].relay_url, WRITE_RELAY);
    let card = kernel.profile_card_for(&author, "Waiting for kind:0 from indexer");
    assert_eq!(card.display_name.as_deref(), Some("Alice"));
}
