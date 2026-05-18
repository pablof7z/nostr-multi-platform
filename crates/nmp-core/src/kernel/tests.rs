use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::InsertOutcome;

#[test]
fn open_author_emits_profile_and_note_reqs() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let requests = kernel.open_author(FIATJAF_PUBKEY.to_string(), true);

    assert_eq!(requests.len(), 3);
    let joined = requests
        .iter()
        .map(|request| request.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(requests
        .iter()
        .any(|request| request.role == RelayRole::Indexer));
    assert!(requests
        .iter()
        .any(|request| request.role == RelayRole::Content));
    assert!(joined.contains("\"author-relays-1\""));
    assert!(joined.contains("\"author-profile-1\""));
    assert!(joined.contains("\"author-notes-1\""));
    assert!(joined.contains("\"kinds\":[10002]"));
    assert!(joined.contains("\"kinds\":[0]"));
    assert!(joined.contains("\"kinds\":[1,6]"));
    assert!(joined.contains(FIATJAF_PUBKEY));
    assert!(!kernel.author_request_pending);
}

#[test]
fn open_thread_emits_context_and_reply_reqs() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let focused_id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let root_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let previous_id = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    kernel.events.insert(
        focused_id.to_string(),
        StoredEvent {
            id: focused_id.to_string(),
            author: TEST_PUBKEY.to_string(),
            kind: 1,
            created_at: 1,
            tags: vec![
                vec![
                    "e".to_string(),
                    root_id.to_string(),
                    "".to_string(),
                    "root".to_string(),
                ],
                vec![
                    "e".to_string(),
                    previous_id.to_string(),
                    "".to_string(),
                    "reply".to_string(),
                ],
            ],
            content: "focused".to_string(),
            relay_count: 1,
        },
    );

    let requests = kernel.open_thread(focused_id.to_string(), true);

    assert_eq!(requests.len(), 2);
    let joined = requests
        .iter()
        .map(|request| request.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("\"thread-ids-1\""));
    assert!(joined.contains("\"thread-replies-2\""));
    assert!(joined.contains(focused_id));
    assert!(joined.contains(root_id));
    assert!(joined.contains(previous_id));
    assert!(joined.contains("\"#e\""));
    assert!(!kernel.thread_request_pending);
}

#[test]
fn close_author_refcounts_and_closes_view_subscriptions() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let _ = kernel.open_author(FIATJAF_PUBKEY.to_string(), true);
    let _ = kernel.open_author(FIATJAF_PUBKEY.to_string(), true);

    let first_close = kernel.close_author(FIATJAF_PUBKEY);
    assert!(first_close.is_empty());
    assert_eq!(
        kernel.selected_author.as_ref().map(|view| view.refcount),
        Some(1)
    );

    let second_close = kernel.close_author(FIATJAF_PUBKEY);
    let joined = second_close
        .iter()
        .map(|request| request.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("\"CLOSE\""));
    assert!(joined.contains("author-profile-1"));
    assert!(joined.contains("author-notes-1"));
    assert!(kernel.selected_author.is_none());
}

#[test]
fn profile_claims_are_ui_driven_and_deduped_by_pubkey() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let first = kernel.claim_profile(
        FIATJAF_PUBKEY.to_string(),
        "timeline-row:first".to_string(),
        true,
    );
    let second = kernel.claim_profile(
        FIATJAF_PUBKEY.to_string(),
        "timeline-row:second".to_string(),
        true,
    );

    assert_eq!(first.len(), 1);
    assert!(second.is_empty());
    assert!(first[0].text.contains("\"profile-claim-1\""));
    assert!(first[0].text.contains("\"kinds\":[0]"));
    assert!(first[0].text.contains(FIATJAF_PUBKEY));
    assert_eq!(
        kernel
            .profile_claims
            .get(FIATJAF_PUBKEY)
            .map(|claims| claims.len()),
        Some(2)
    );

    let first_release = kernel.release_profile(FIATJAF_PUBKEY, "timeline-row:first");
    assert!(first_release.is_empty());
    assert_eq!(
        kernel
            .profile_claims
            .get(FIATJAF_PUBKEY)
            .map(|claims| claims.len()),
        Some(1)
    );

    let second_release = kernel.release_profile(FIATJAF_PUBKEY, "timeline-row:second");
    assert!(second_release.is_empty());
    assert!(!kernel.profile_claims.contains_key(FIATJAF_PUBKEY));
}

#[test]
fn parse_relay_list_splits_nip65_markers() {
    let parsed = parse_relay_list(
        "deadbeef",
        123,
        &[
            vec![
                "r".to_string(),
                "wss://read.example".to_string(),
                "read".to_string(),
            ],
            vec![
                "r".to_string(),
                "wss://write.example".to_string(),
                "write".to_string(),
            ],
            vec!["r".to_string(), "wss://both.example".to_string()],
            vec![
                "r".to_string(),
                "https://not-a-relay.example".to_string(),
                "read".to_string(),
            ],
        ],
    );

    assert_eq!(parsed.created_at, 123);
    assert_eq!(parsed.read_relays, vec!["wss://read.example"]);
    assert_eq!(parsed.write_relays, vec!["wss://write.example"]);
    assert_eq!(parsed.both_relays, vec!["wss://both.example"]);
}

// ─── D4 regression tests: stale re-delivery must not overwrite local cache ───

const PK_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ID_V1: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const ID_V2: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const RELAY: &str = "wss://test.relay/";

/// D4 — kind:3 regression: deliver v2 then re-deliver stale v1.
///
/// The store must supersede v1 (older created_at) and the kernel's
/// `seed_contacts` cache must stay at the v2 content.
#[test]
fn kind3_stale_redelivery_does_not_overwrite_contacts_cache() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // v2 — newer event with two follows.
    let follows_v2: Vec<Vec<String>> = vec![
        vec!["p".to_string(), FIATJAF_PUBKEY.to_string()],
        vec!["p".to_string(), JB55_PUBKEY.to_string()],
    ];
    let o2 = kernel
        .inject_replaceable_event(ID_V2, PK_A, 2000, 3, follows_v2, RELAY, 2_000_000)
        .expect("store insert must succeed");
    assert!(
        matches!(o2, InsertOutcome::Inserted { .. }),
        "v2 must be freshly inserted, got {o2:?}"
    );
    let contacts_after_v2 = kernel
        .seed_contacts
        .get(PK_A)
        .cloned()
        .expect("contacts must be populated after v2");
    assert_eq!(
        contacts_after_v2.len(),
        2,
        "cache should hold v2's two follows"
    );

    // v1 — older event with one follow (stale re-delivery).
    let follows_v1: Vec<Vec<String>> = vec![vec!["p".to_string(), FIATJAF_PUBKEY.to_string()]];
    let o1 = kernel
        .inject_replaceable_event(ID_V1, PK_A, 1000, 3, follows_v1, RELAY, 1_000_000)
        .expect("store insert must succeed");
    assert!(
        matches!(o1, InsertOutcome::Superseded { .. }),
        "stale v1 must be Superseded by the store, got {o1:?}"
    );

    // Cache must still reflect v2 — the stale v1 must not have overwritten it.
    let contacts_after_v1 = kernel
        .seed_contacts
        .get(PK_A)
        .cloned()
        .expect("contacts must still be populated");
    assert_eq!(
        contacts_after_v1.len(),
        2,
        "D4 violation: stale v1 overwrote v2 contacts cache"
    );
}

/// D4 — kind:10002 regression: deliver v2 then re-deliver stale v1.
///
/// The store must supersede v1 and the kernel's `author_relay_lists`
/// cache must stay at the v2 relay list.
#[test]
fn kind10002_stale_redelivery_does_not_overwrite_relay_list_cache() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // v2 — two relays.
    let tags_v2: Vec<Vec<String>> = vec![
        vec![
            "r".to_string(),
            "wss://v2-read.example/".to_string(),
            "read".to_string(),
        ],
        vec![
            "r".to_string(),
            "wss://v2-write.example/".to_string(),
            "write".to_string(),
        ],
    ];
    let o2 = kernel
        .inject_replaceable_event(ID_V2, PK_A, 2000, 10002, tags_v2, RELAY, 2_000_000)
        .expect("store insert must succeed");
    assert!(
        matches!(o2, InsertOutcome::Inserted { .. }),
        "v2 must be freshly inserted, got {o2:?}"
    );
    let list_after_v2 = kernel
        .author_relay_lists
        .get(PK_A)
        .cloned()
        .expect("relay list must be populated after v2");
    assert_eq!(
        list_after_v2.created_at, 2000,
        "cache should hold v2 timestamp"
    );

    // v1 — older event with one relay.
    let tags_v1: Vec<Vec<String>> =
        vec![vec!["r".to_string(), "wss://v1-only.example/".to_string()]];
    let o1 = kernel
        .inject_replaceable_event(ID_V1, PK_A, 1000, 10002, tags_v1, RELAY, 1_000_000)
        .expect("store insert must succeed");
    assert!(
        matches!(o1, InsertOutcome::Superseded { .. }),
        "stale v1 must be Superseded by the store, got {o1:?}"
    );

    // Cache must still reflect v2's timestamp.
    let list_after_v1 = kernel
        .author_relay_lists
        .get(PK_A)
        .cloned()
        .expect("relay list must still be populated");
    assert_eq!(
        list_after_v1.created_at, 2000,
        "D4 violation: stale v1 overwrote v2 relay list cache"
    );
}

// ─── C13 kernel companion: D1 placeholder contract + in-place refinement ─────

const C13_PK: &str = "c13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13a";
const C13_ID: &str = "e1e2e3e4e5e6e7e8e9eae1e2e3e4e5e6e7e8e9eae1e2e3e4e5e6e7e8e9eae1e2";
const C13_KIND0_ID: &str = "f1f2f3f4f5f6f7f8f9faf1f2f3f4f5f6f7f8f9faf1f2f3f4f5f6f7f8f9faf1f2";

/// C13 kernel companion (D1 best-effort rendering — placeholder contract).
///
/// Phase 1: before any kind:0 arrives, `timeline_item().author_picture_url`
/// must be a non-empty deterministic identicon URI (never empty, never panic).
///
/// Phase 2: after kind:0 with a real picture URL arrives, the same item's
/// `author_picture_url` must resolve to the real URL (in-place refinement).
///
/// Design: `docs/product-spec/doctrine.md` §D1, ADR-0017.
#[test]
fn c13_kernel_timeline_item_d1_picture_url_placeholder_and_refinement() {
    use crate::substrate::placeholder::picture_placeholder;
    use crate::store::VerifiedEvent;

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // ── Phase 1: inject kind:1, no kind:0 yet ────────────────────────────────
    let raw_note = crate::store::RawEvent {
        id: C13_ID.to_string(),
        pubkey: C13_PK.to_string(),
        created_at: 1_000,
        kind: 1,
        tags: vec![],
        content: "test note".to_string(),
        sig: "a".repeat(128),
    };
    kernel.ingest_pre_verified_event(
        crate::relay::RelayRole::Content,
        "diag-firehose-stress",
        VerifiedEvent::from_raw_unchecked(raw_note),
    );
    kernel.sort_timeline_deferred();

    let event = kernel.events.get(C13_ID).expect("event must be in cache");
    let item_no_profile = kernel.timeline_item(event);

    // author_picture_url is a String (not Option) and never empty (D1).
    assert!(
        !item_no_profile.author_picture_url.is_empty(),
        "D1 violation: author_picture_url must never be empty before kind:0"
    );
    assert!(
        item_no_profile.author_picture_url.starts_with("identicon:"),
        "placeholder must be an identicon URI, got: {}",
        item_no_profile.author_picture_url
    );
    assert_eq!(
        item_no_profile.author_picture_url,
        picture_placeholder(C13_PK),
        "placeholder must be deterministic"
    );
    assert_eq!(item_no_profile.author_avatar_source, "placeholder");

    // ── Phase 2: inject kind:0 with a real picture URL ───────────────────────
    let picture = "https://example.com/avatar.png";

    // Insert the profile directly into the kernel's profile cache.
    // (inject_replaceable_event always uses empty content and therefore
    //  produces a profile with no picture_url — insufficient for this test.)
    kernel.profiles.insert(C13_PK.to_string(), Profile {
        event_id: C13_KIND0_ID.to_string(),
        created_at: 2_000,
        display: "c13".to_string(),
        picture_url: Some(picture.to_string()),
        nip05: String::new(),
        about: String::new(),
        avatar_initials: "c1".to_string(),
        avatar_color: avatar_color(C13_PK),
    });

    let event_after = kernel.events.get(C13_ID).expect("event must still be in cache");
    let item_with_profile = kernel.timeline_item(event_after);

    // After kind:0 arrives, the real picture URL replaces the placeholder.
    assert_eq!(
        item_with_profile.author_picture_url, picture,
        "D1 refinement: picture_url must update to kind:0 URL in place"
    );
    assert_eq!(item_with_profile.author_avatar_source, "kind0");
    // The item id is unchanged — refinement is in-place.
    assert_eq!(item_with_profile.id, item_no_profile.id);
}

/// P2 — kind:10002 empty relay list clears the cache entry.
///
/// When a canonical *newer* kind:10002 carries an empty relay list, the author
/// has explicitly cleared their NIP-65 metadata.  The old cache entry must be
/// *removed* rather than left stale.
#[test]
fn kind10002_empty_relay_list_clears_cache_entry() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // v1 — non-empty relay list; populates the cache.
    let tags_v1: Vec<Vec<String>> = vec![
        vec![
            "r".to_string(),
            "wss://v1-read.example/".to_string(),
            "read".to_string(),
        ],
        vec![
            "r".to_string(),
            "wss://v1-write.example/".to_string(),
            "write".to_string(),
        ],
    ];
    let o1 = kernel
        .inject_replaceable_event(ID_V1, PK_A, 1000, 10002, tags_v1, RELAY, 1_000_000)
        .expect("v1 store insert must succeed");
    assert!(
        matches!(o1, InsertOutcome::Inserted { .. }),
        "v1 must be freshly inserted, got {o1:?}"
    );
    assert!(
        kernel.author_relay_lists.contains_key(PK_A),
        "cache must be populated after v1"
    );

    // v2 — newer event with an EMPTY relay list (author cleared NIP-65).
    let o2 = kernel
        .inject_replaceable_event(ID_V2, PK_A, 2000, 10002, vec![], RELAY, 2_000_000)
        .expect("v2 store insert must succeed");
    assert!(
        matches!(
            o2,
            InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. }
        ),
        "v2 must supersede v1 in the store, got {o2:?}"
    );

    // Cache entry must be removed — empty list clears the stale relay metadata.
    assert!(
        !kernel.author_relay_lists.contains_key(PK_A),
        "empty kind:10002 must remove stale cache entry"
    );
}
