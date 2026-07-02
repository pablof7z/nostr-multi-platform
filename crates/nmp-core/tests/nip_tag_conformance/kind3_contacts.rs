//! Kind:3 contact lists (NIP-02) — follow and unfollow tag structure.

use crate::support::*;

/// NIP-02: a kind:3 contact list carries one `p` tag per followed pubkey — and
/// nothing else. This test seeds an existing follow set, adds one, and asserts
/// the re-published list is exactly the union, every `p` value a 64-hex pubkey.
#[test]
fn kind3_contacts_carry_one_p_tag_per_followed_pubkey() {
    let mut h = signed_harness();
    let author = h.active_pubkey().expect("signed in");
    let existing_a = hex64('2');
    let existing_b = hex64('3');
    let newly_followed = hex64('4');
    h.seed_contact_list(&author, &[&existing_a, &existing_b]);

    let event = h.emit_follow(&newly_followed, true);
    assert_eq!(event["kind"], 3, "contact list must be kind:3");

    let mut p_values = values_for_key(&event, "p");
    p_values.sort();
    let mut expected = vec![
        existing_a.clone(),
        existing_b.clone(),
        newly_followed.clone(),
    ];
    expected.sort();
    assert_eq!(
        p_values, expected,
        "NIP-02 kind:3 must carry exactly one `p` tag per followed pubkey (the union)"
    );

    // Every `p` value must be a well-formed 64-hex pubkey.
    for tag in tags_with_key(&event, "p") {
        let pubkey = tag.get(1).expect("`p` tag has a value column");
        assert!(
            is_hex64(pubkey),
            "every NIP-02 `p` value must be a 64-hex pubkey, got: {pubkey:?}"
        );
    }

    // A contact list carries `p` tags and nothing else.
    assert_only_keys(&event, &["p"], "NIP-02 contact list");
}

/// NIP-02: unfollow removes exactly the named pubkey and re-publishes the rest —
/// the kind:3 must not retain a stale `p` tag for the dropped pubkey.
#[test]
fn kind3_unfollow_drops_exactly_one_p_tag() {
    let mut h = signed_harness();
    let author = h.active_pubkey().expect("signed in");
    let keep = hex64('5');
    let drop = hex64('6');
    h.seed_contact_list(&author, &[&keep, &drop]);

    let event = h.emit_follow(&drop, false);
    let p_values = values_for_key(&event, "p");
    assert_eq!(
        p_values,
        vec![keep.clone()],
        "NIP-02 unfollow must drop exactly the named `p` tag, keep the rest"
    );
    assert_only_keys(&event, &["p"], "NIP-02 contact list after unfollow");
}
