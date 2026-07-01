//! Browser reactivity gates for NIP-51 simple-group feed sources.

mod support;

use support::*;

const LIST_RELAY: &str = "wss://lists.example";
const GROUP_RELAY_A: &str = "wss://groups-a.example";
const GROUP_RELAY_B: &str = "wss://groups-b.example";
const FEED_KEY_REPLACE: &str = "test.browser.feed.simple_groups.replace";
const FEED_KEY_SWITCH: &str = "test.browser.feed.simple_groups.switch";

#[test]
fn simple_group_feed_replaces_group_list_without_app_intervention() {
    let viewer_keys = nostr::Keys::generate();
    let author_a_keys = nostr::Keys::generate();
    let author_b_keys = nostr::Keys::generate();
    let viewer_pk = viewer_keys.public_key().to_hex();

    let mut handle = started_group_feed();
    open_simple_group_feed(&mut handle, FEED_KEY_REPLACE);

    let outbound = handle.apply_set_active_account(viewer_pk.clone());
    handle.fan_out_outbound(outbound);
    let after_sign_in = handle.pump();
    let list_sub = req_sub_for_kind_author_on(
        &after_sign_in.outbound,
        LIST_RELAY,
        nmp_kinds::KIND_SIMPLE_GROUPS,
        &viewer_pk,
    )
    .expect("active account must open its simple-groups list subscription");

    deliver(
        &mut handle,
        LIST_RELAY,
        &list_sub,
        signed_simple_groups_json(&viewer_keys, &[("room-a", GROUP_RELAY_A)], 10),
    );
    let after_list_a = handle.pump();
    let group_sub_a = req_sub_for_kind_h_on(
        &after_list_a.outbound,
        GROUP_RELAY_A,
        nmp_kinds::KIND_SHORT_TEXT_NOTE,
        "room-a",
    )
    .expect("room-a list entry must open a host-pinned group subscription");

    let note_a_json = signed_group_note_json(&author_a_keys, "room-a", "room A note", 20);
    let note_a_id = event_id_from_json(&note_a_json);
    deliver(&mut handle, GROUP_RELAY_A, &group_sub_a, note_a_json);
    let feed = decode_feed(&handle.next_frame(true), FEED_KEY_REPLACE);
    assert!(
        feed.cards.iter().any(|card| card.card.id == note_a_id),
        "listed room-a note must render before replacement"
    );

    deliver(
        &mut handle,
        LIST_RELAY,
        &list_sub,
        signed_simple_groups_json(&viewer_keys, &[("room-b", GROUP_RELAY_B)], 30),
    );
    let after_list_b = handle.pump();
    let group_sub_b = req_sub_for_kind_h_on(
        &after_list_b.outbound,
        GROUP_RELAY_B,
        nmp_kinds::KIND_SHORT_TEXT_NOTE,
        "room-b",
    )
    .expect("replacement list entry must open room-b on its host relay");
    assert!(
        req_sub_for_kind_h_on(
            &after_list_b.outbound,
            GROUP_RELAY_A,
            nmp_kinds::KIND_SHORT_TEXT_NOTE,
            "room-a",
        )
        .is_none(),
        "replacement demand must not reopen the withdrawn room-a host interest"
    );

    let feed = decode_feed(&handle.next_frame(true), FEED_KEY_REPLACE);
    assert!(
        feed.cards.iter().all(|card| card.card.id != note_a_id),
        "simple-groups replacement must reset rows from the old group set"
    );

    let note_b_json = signed_group_note_json(&author_b_keys, "room-b", "room B note", 40);
    let note_b_id = event_id_from_json(&note_b_json);
    deliver(&mut handle, GROUP_RELAY_B, &group_sub_b, note_b_json);
    let feed = decode_feed(&handle.next_frame(true), FEED_KEY_REPLACE);
    assert!(
        feed.cards.iter().any(|card| card.card.id == note_b_id),
        "newly listed room-b note must render after replacement"
    );
    assert!(
        feed.cards.iter().all(|card| card.card.id != note_a_id),
        "old group rows must stay absent after replacement renders"
    );
}

#[test]
fn simple_group_feed_retargets_on_account_switch_without_app_intervention() {
    let viewer_one_keys = nostr::Keys::generate();
    let viewer_two_keys = nostr::Keys::generate();
    let author_a_keys = nostr::Keys::generate();
    let author_b_keys = nostr::Keys::generate();
    let viewer_one_pk = viewer_one_keys.public_key().to_hex();
    let viewer_two_pk = viewer_two_keys.public_key().to_hex();

    let mut handle = started_group_feed();
    open_simple_group_feed(&mut handle, FEED_KEY_SWITCH);

    let outbound = handle.apply_set_active_account(viewer_one_pk.clone());
    handle.fan_out_outbound(outbound);
    let after_viewer_one = handle.pump();
    let list_sub_one = req_sub_for_kind_author_on(
        &after_viewer_one.outbound,
        LIST_RELAY,
        nmp_kinds::KIND_SIMPLE_GROUPS,
        &viewer_one_pk,
    )
    .expect("viewer one must open its simple-groups list subscription");

    deliver(
        &mut handle,
        LIST_RELAY,
        &list_sub_one,
        signed_simple_groups_json(&viewer_one_keys, &[("room-a", GROUP_RELAY_A)], 10),
    );
    let after_list_one = handle.pump();
    let group_sub_one = req_sub_for_kind_h_on(
        &after_list_one.outbound,
        GROUP_RELAY_A,
        nmp_kinds::KIND_SHORT_TEXT_NOTE,
        "room-a",
    )
    .expect("viewer one's room-a source must open");

    let note_a_json = signed_group_note_json(&author_a_keys, "room-a", "viewer one room", 20);
    let note_a_id = event_id_from_json(&note_a_json);
    deliver(&mut handle, GROUP_RELAY_A, &group_sub_one, note_a_json);
    let feed = decode_feed(&handle.next_frame(true), FEED_KEY_SWITCH);
    assert!(
        feed.cards.iter().any(|card| card.card.id == note_a_id),
        "viewer one's listed group note must render before the switch"
    );

    let outbound = handle.apply_set_active_account(viewer_two_pk.clone());
    handle.fan_out_outbound(outbound);
    let after_switch = handle.pump();
    let list_sub_two = req_sub_for_kind_author_on(
        &after_switch.outbound,
        LIST_RELAY,
        nmp_kinds::KIND_SIMPLE_GROUPS,
        &viewer_two_pk,
    )
    .expect("active-account switch must retarget the simple-groups resolver");

    let feed = decode_feed(&handle.next_frame(true), FEED_KEY_SWITCH);
    assert!(
        feed.cards.iter().all(|card| card.card.id != note_a_id),
        "account switch must reset rendered rows from the prior account's groups"
    );

    deliver(
        &mut handle,
        LIST_RELAY,
        &list_sub_two,
        signed_simple_groups_json(&viewer_two_keys, &[("room-b", GROUP_RELAY_B)], 30),
    );
    let after_list_two = handle.pump();
    let group_sub_two = req_sub_for_kind_h_on(
        &after_list_two.outbound,
        GROUP_RELAY_B,
        nmp_kinds::KIND_SHORT_TEXT_NOTE,
        "room-b",
    )
    .expect("viewer two's room-b source must open");

    let note_b_json = signed_group_note_json(&author_b_keys, "room-b", "viewer two room", 40);
    let note_b_id = event_id_from_json(&note_b_json);
    deliver(&mut handle, GROUP_RELAY_B, &group_sub_two, note_b_json);
    let feed = decode_feed(&handle.next_frame(true), FEED_KEY_SWITCH);
    assert!(
        feed.cards.iter().any(|card| card.card.id == note_b_id),
        "viewer two's listed group note must render after the switch"
    );
    assert!(
        feed.cards.iter().all(|card| card.card.id != note_a_id),
        "viewer one's group rows must not survive the active-account switch"
    );
}
