//! Local replaceable-event publish projection tests.

use super::*;
use crate::publish::PublishTarget;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::substrate::{SignedEvent, UnsignedEvent};

const FOLLOWED: &str = "1111111111111111111111111111111111111111111111111111111111111111";

fn snapshot(kernel: &mut Kernel) -> serde_json::Value {
    serde_json::from_str(&kernel.make_update_json_for_test(true))
        .expect("kernel snapshot must be valid JSON")
}

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

#[test]
fn local_kind3_publish_updates_profile_action_from_contacts_projection() {
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let signed = signed_contact_list(&keys, FOLLOWED, 1_700_000_000);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(author.clone());
    kernel.seed_kind10002_for_test(&author, &["wss://write.test"]);
    kernel.open_author(FOLLOWED.to_string(), std::collections::BTreeSet::from([1u32, 6u32]), false);

    // D0: the author view is no longer a typed `KernelSnapshot.author_view`
    // field — it is a built-in entry in the `projections` map under the key
    // `"author_view"`.
    assert_eq!(
        snapshot(&mut kernel)["projections"]["author_view"]["primary_action"]["kind"].as_str(),
        Some("follow")
    );

    let outbound = kernel.run_publish_engine_at(&signed, &[], PublishTarget::Auto, None, 1_000);

    assert!(!outbound.is_empty(), "publish should have an outbox target");
    assert_eq!(
        snapshot(&mut kernel)["projections"]["author_view"]["primary_action"]["kind"].as_str(),
        Some("unfollow")
    );
}
