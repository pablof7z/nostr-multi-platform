//! Local replaceable-event publish projection tests.

use super::*;
use crate::publish::PublishTarget;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::substrate::{SignedEvent, UnsignedEvent};

const FOLLOWED: &str = "1111111111111111111111111111111111111111111111111111111111111111";

fn signed_contact_list(keys: &::nostr::Keys, follow: &str, created_at: u64) -> SignedEvent {
    let event = ::nostr::EventBuilder::new(::nostr::Kind::from(3u16), "")
        .tags([::nostr::Tag::parse(["p", follow]).expect("valid p tag")])
        .custom_created_at(::nostr::Timestamp::from_secs(created_at))
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
                .map(|tag: &::nostr::Tag| tag.as_slice().to_vec())
                .collect(),
            content: event.content.clone(),
            created_at: event.created_at.as_secs(),
        },
    }
}

/// V-112 (ADR-0042): `author_view.primary_action` was deleted with the author
/// view state machine. The underlying property being tested — that publishing a
/// kind:3 contact list updates `kernel.contacts` — is now observed directly.
#[test]
fn local_kind3_publish_updates_contacts_set() {
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let signed = signed_contact_list(&keys, FOLLOWED, 1_700_000_000);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(author.clone());
    kernel.seed_kind10002_for_test(&author, &["wss://write.test"]);

    // Before publishing kind:3, FOLLOWED is not in seed_contacts for this author.
    assert!(
        kernel
            .seed_contacts
            .get(&author)
            .map_or(true, |follows| !follows.contains(&FOLLOWED.to_string())),
        "precondition: FOLLOWED must not be in seed_contacts before publish"
    );

    let outbound = kernel.run_publish_engine_at(&signed, &[], PublishTarget::Auto, None, 1_000);

    assert!(!outbound.is_empty(), "publish should have an outbox target");
    // After publishing kind:3 with FOLLOWED in the p-tags, seed_contacts is updated.
    assert!(
        kernel
            .seed_contacts
            .get(&author)
            .map_or(false, |follows| follows.contains(&FOLLOWED.to_string())),
        "FOLLOWED must be in seed_contacts[author] after kind:3 publish"
    );
}
