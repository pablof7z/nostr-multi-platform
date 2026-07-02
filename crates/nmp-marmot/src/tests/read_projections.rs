//! Read projections: `get_groups` / `get_messages` / `group_leaf_map`.

use nostr::{EventBuilder, Kind, PublicKey};

use super::fixtures::{bootstrap_pair, new_actor};

/// The read projections that back the Domain/View modules must reflect the
/// real MLS state: `get_groups` lists the created group, `get_messages`
/// returns delivered application messages, and `group_leaf_map` is keyed by
/// the exact same pubkey set as `get_members`.
#[test]
fn read_projections_reflect_group_state() {
    let alice = new_actor();
    let bob = new_actor();
    let group_id = bootstrap_pair(&alice, &bob);

    // get_groups lists exactly the one created group.
    let groups = alice.service.get_groups().expect("get_groups");
    assert_eq!(groups.len(), 1, "exactly one group");
    assert_eq!(groups[0].mls_group_id, group_id);

    // group_leaf_map's pubkey set equals get_members.
    let members = alice.service.get_members(&group_id).unwrap();
    let leaf_map = alice.service.group_leaf_map(&group_id).expect("leaf map");
    let leaf_pubkeys: std::collections::BTreeSet<PublicKey> = leaf_map.values().cloned().collect();
    assert_eq!(
        leaf_pubkeys, members,
        "leaf map pubkeys must match the member set"
    );

    // An application message round-trips into get_messages on the receiver.
    let rumor = EventBuilder::new(Kind::TextNote, "history check").build(alice.pubkey());
    let msg = alice
        .service
        .create_message(&group_id, rumor)
        .expect("alice creates message");
    bob.service
        .process_message(&msg)
        .expect("bob processes message");
    let history = bob.service.get_messages(&group_id).expect("get_messages");
    assert!(
        history.iter().any(|m| m.content == "history check"),
        "delivered message must surface in bob's get_messages projection"
    );
}
