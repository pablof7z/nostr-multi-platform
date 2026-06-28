use super::*;

#[test]
fn replaceable_matches_nip01_and_nostr() {
    // Regular replaceable per NIP-01 + nostr's kind:41 special case.
    assert!(is_replaceable(0), "kind:0 metadata");
    assert!(is_replaceable(3), "kind:3 contacts");
    assert!(is_replaceable(41), "kind:41 NIP-28 channel metadata");
    assert!(is_replaceable(10_000), "kind:10000 mute list");
    assert!(is_replaceable(10_002), "kind:10002 relay list");
    assert!(is_replaceable(10_007), "kind:10007 search relays");
    assert!(is_replaceable(19_999), "kind:19999 end of range");

    // The DIVERGENT-bug cases: notes/reposts/reactions are NOT replaceable.
    assert!(!is_replaceable(1), "kind:1 short text note is regular");
    assert!(!is_replaceable(6), "kind:6 repost is regular");
    assert!(!is_replaceable(7), "kind:7 reaction is regular");
    assert!(!is_replaceable(9_999), "kind:9999 end of regular range");

    // Ephemeral + addressable are not regular-replaceable.
    assert!(!is_replaceable(20_000), "kind:20000 ephemeral");
    assert!(!is_replaceable(29_999), "kind:29999 ephemeral");
    assert!(!is_replaceable(30_000), "kind:30000 addressable");
    assert!(!is_replaceable(40_000), "kind:40000 above addressable");
}

#[test]
fn content_render_kind_constants_match_protocol_numbers() {
    assert_eq!(KIND_LONG_FORM_ARTICLE, 30_023);
    assert_eq!(KIND_LONG_FORM_DRAFT, 30_024);
    assert_eq!(KIND_WIKI_ARTICLE, 30_818);
    assert_eq!(KIND_BOOKMARK_SET, 30_003);
    assert_eq!(KIND_ARTICLE_CURATION_SET, 30_004);
    assert_eq!(KIND_WEB_BOOKMARK, 39_701);
}

#[test]
fn addressable_range() {
    assert!(is_addressable(30_000), "start of range");
    assert!(is_addressable(30_023), "long-form article");
    assert!(is_addressable(39_999), "end of range");

    // Ephemeral is NOT addressable (prior copy wrongly said it was).
    assert!(!is_addressable(20_000), "ephemeral start");
    assert!(!is_addressable(29_999), "ephemeral end");

    // Neither regular nor regular-replaceable kinds are addressable.
    assert!(!is_addressable(0));
    assert!(!is_addressable(3));
    assert!(!is_addressable(10_000));
    assert!(!is_addressable(40_000));
}

#[test]
fn ephemeral_range() {
    assert!(is_ephemeral(20_000), "start of range");
    assert!(is_ephemeral(29_999), "end of range");

    assert!(!is_ephemeral(19_999), "below ephemeral range");
    assert!(!is_ephemeral(30_000), "addressable start");
    assert!(!is_ephemeral(40_000), "above addressable");
    assert!(!is_ephemeral(0));
    assert!(!is_ephemeral(3));
    assert!(!is_ephemeral(10_000));
}

#[test]
fn ptags_are_recipients_classifies_lists_as_subjects() {
    // Replaceable list/discovery kinds: p-tags are subjects, not recipients.
    assert!(
        !ptags_are_recipients(3),
        "kind:3 contact list p-tags are followees (subjects)"
    );
    assert!(
        !ptags_are_recipients(0),
        "kind:0 profile (no p-tags, but still replaceable)"
    );
    assert!(
        !ptags_are_recipients(10_000),
        "kind:10000 mute list p-tags are muted pubkeys (subjects)"
    );
    assert!(
        !ptags_are_recipients(10_002),
        "kind:10002 relay list - replaceable, no recipient p-tags"
    );
    assert!(
        !ptags_are_recipients(41),
        "kind:41 NIP-28 channel metadata - replaceable"
    );

    // Addressable list/set kinds: p-tags are subjects, not recipients.
    assert!(
        !ptags_are_recipients(30_000),
        "kind:30000 follow set p-tags are list members (subjects)"
    );
    assert!(
        !ptags_are_recipients(30_023),
        "kind:30023 long-form article - addressable"
    );
    assert!(!ptags_are_recipients(39_999), "end of addressable range");

    // Regular events: p-tags are recipients.
    assert!(
        ptags_are_recipients(1),
        "kind:1 short text note mentions are recipients"
    );
    assert!(
        ptags_are_recipients(7),
        "kind:7 reaction - recipient fan-out enabled"
    );
    assert!(
        ptags_are_recipients(1059),
        "kind:1059 gift-wrap - recipient routing semantics"
    );
}

#[test]
fn encrypted_content_kinds_are_ciphertext_only() {
    // Ciphertext-content kinds: content must be hidden.
    assert!(is_encrypted_content_kind(4), "kind:4 NIP-04 DM");
    assert!(is_encrypted_content_kind(13), "kind:13 NIP-59 seal");
    assert!(is_encrypted_content_kind(44), "kind:44 legacy versioned DM");
    assert!(
        is_encrypted_content_kind(KIND_GIFT_WRAP),
        "kind:1059 gift-wrap"
    );
    assert!(
        is_encrypted_content_kind(1060),
        "kind:1060 legacy gift-wrap"
    );

    // NIP-17 rumors are plaintext content but relay-presence private.
    assert!(
        !is_encrypted_content_kind(KIND_CHAT_MESSAGE),
        "kind:14 NIP-17 chat rumor content is decrypted plaintext"
    );
    assert!(
        !is_encrypted_content_kind(15),
        "kind:15 NIP-17 file rumor content is plaintext"
    );

    assert!(!is_encrypted_content_kind(0), "kind:0 profile metadata");
    assert!(!is_encrypted_content_kind(1), "kind:1 short text note");
    assert!(!is_encrypted_content_kind(7), "kind:7 reaction");
    assert!(
        !is_encrypted_content_kind(30_023),
        "kind:30023 long-form article"
    );
}

#[test]
fn private_relay_provenance_kinds_hide_metadata_presence() {
    for kind in PRIVATE_RELAY_PROVENANCE_KINDS {
        assert!(
            is_private_relay_provenance_kind(*kind),
            "kind {kind} must be relay-provenance private"
        );
    }

    assert!(
        is_private_relay_provenance_kind(KIND_CHAT_MESSAGE),
        "kind:14 chat rumors are plaintext but relay-presence private"
    );
    assert!(
        is_private_relay_provenance_kind(15),
        "kind:15 file rumors are plaintext but relay-presence private"
    );
    assert!(
        !is_private_relay_provenance_kind(44),
        "kind:44 remains content-hidden only unless relay policy expands"
    );
    assert!(!is_private_relay_provenance_kind(1), "public note");
    assert!(!is_private_relay_provenance_kind(30_023), "public article");
}
