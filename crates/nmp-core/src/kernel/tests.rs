use super::*;
use crate::kernel::refs::{ProfileShape, RefLiveness, RefNamespace, RefShape};
use crate::display::avatar_color_hex;
use crate::relay::{DEFAULT_VISIBLE_LIMIT, FIATJAF_PUBKEY, JB55_PUBKEY};
use crate::store::InsertOutcome;

// V-68 / V-112 (ADR-0042): open_author_emits_profile_and_note_reqs,
// open_author_with_cached_nip65_routes_notes_to_resolved_write_relays,
// open_thread_emits_context_and_reply_reqs,
// close_author_refcounts_and_closes_view_subscriptions,
// close_thread_refcounts_and_closes_view_subscriptions,
// v68_thread_reply_req_carries_host_supplied_kinds_1_6,
// v68_thread_reply_req_carries_arbitrary_host_kinds,
// v68_deferred_relay_path_reads_stored_kinds,
// v68_author_note_req_carries_host_supplied_kinds_1_6,
// v68_author_note_req_carries_sentinel_kind_not_hardcoded,
// v68_author_empty_kinds_emits_no_notes_req,
// v68_author_deferred_relay_path_reads_stored_kinds
// all deleted — open_author / open_thread / close_author / close_thread and
// their state structs (AuthorViewState / ThreadViewState) removed from kernel.
// Per-app FlatFeed in nmp-app-chirp now owns this behavior.

#[test]
fn profile_claims_are_ui_driven_and_deduped_by_pubkey() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    kernel.relay_connected(RelayRole::Indexer);

    // M2 migration: a profile resolve_ref registers a kind:0 interest and
    // returns empty (the planner emits the wire REQ on drain). Two consumers of
    // one pubkey dedup to ONE interest, keeping `profile_claims` refcount at 2.
    let mut resolve = |consumer: &str| {
        kernel.resolve_ref(
            RefNamespace::Profile,
            FIATJAF_PUBKEY.to_string(),
            consumer.to_string(),
            RefShape::Profile(ProfileShape::Card),
            RefLiveness::CacheOk.into(),
            false,
            Vec::new(),
        )
    };
    let first = resolve("timeline-row:first");
    let second = resolve("timeline-row:second");
    assert!(first.is_empty(), "profile resolve emits no outbound directly");
    assert!(second.is_empty());

    // The planner emits a kind:0 REQ for the claimed author (detailed routing /
    // batching / probe assertions live in `profile_claim_tests`).
    let reqs: Vec<OutboundMessage> = kernel
        .drain_lifecycle_outbound()
        .into_iter()
        .filter(|m| m.text.starts_with("[\"REQ\""))
        .collect();
    assert!(
        reqs.iter()
            .any(|m| m.text.contains("\"kinds\":[0]") && m.text.contains(FIATJAF_PUBKEY)),
        "the planner must emit a kind:0 REQ for the claimed author"
    );

    // Two consumers dedup to one interest but the `profile_claims` refcount is 2.
    assert_eq!(
        kernel
            .profile_claims
            .get(FIATJAF_PUBKEY)
            .map(|claims| claims.len()),
        Some(2)
    );

    let first_release = kernel.release_ref(RefNamespace::Profile, FIATJAF_PUBKEY, "timeline-row:first");
    assert!(first_release.is_empty());
    assert_eq!(
        kernel
            .profile_claims
            .get(FIATJAF_PUBKEY)
            .map(|claims| claims.len()),
        Some(1)
    );

    let second_release = kernel.release_ref(RefNamespace::Profile, FIATJAF_PUBKEY, "timeline-row:second");
    assert!(second_release.is_empty());
    assert!(!kernel.profile_claims.contains_key(FIATJAF_PUBKEY));
}

#[test]
fn parse_relay_list_splits_nip65_markers() {
    let parsed = super::nostr::parse_relay_list(&[
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
    ]);

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
/// The store must supersede v1 (older created_at) and the contacts cache
/// must stay at the v2 content.
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
        .contacts_lookup()
        .follows(PK_A)
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
        .contacts_lookup()
        .follows(PK_A)
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
        .mailbox_cache()
        .snapshot(&PK_A.to_string())
        .expect("relay list must be populated after v2");
    // Cache holds v2's relay URLs (the store doesn't surface created_at
    // through the substrate cache — the store itself enforces
    // supersession; the cache is just the projection of the winning
    // event's tags).
    assert_eq!(list_after_v2.read, vec!["wss://v2-read.example/"]);
    assert_eq!(list_after_v2.write, vec!["wss://v2-write.example/"]);

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

    // Cache must still reflect v2's relays (the store rejected v1 so
    // `ingest_relay_list` was never called for v1 — the cache was not
    // touched).
    let list_after_v1 = kernel
        .mailbox_cache()
        .snapshot(&PK_A.to_string())
        .expect("relay list must still be populated");
    assert_eq!(
        list_after_v1.read,
        vec!["wss://v2-read.example/"],
        "D4 violation: stale v1 overwrote v2 relay list cache"
    );
    assert_eq!(list_after_v1.write, vec!["wss://v2-write.example/"]);
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
        kernel.mailbox_cache().known(&PK_A.to_string()),
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
        !kernel.mailbox_cache().known(&PK_A.to_string()),
        "empty kind:10002 must remove stale cache entry"
    );
}

/// V-26 — `Kernel::accounts_enriched` must recompute `avatar_initials` whenever
/// kind:0 metadata replaces the placeholder `display_name`. Otherwise the
/// avatar tile keeps showing the npub-body fallback initials (e.g. `"AB"`
/// from the bech32 body) while the surrounding row text reads `"Alice Smith"`.
/// The Swift extension this V-26 work replaced computed initials at view time
/// so it was implicitly reactive; the Rust-owned field must match that
/// behaviour or the iOS toolbar avatar visibly drifts from the display name.
#[test]
fn accounts_enriched_populates_display_name_when_kind0_lands() {
    use ::nostr::{PublicKey, ToBech32};

    let pubkey_hex = JB55_PUBKEY.to_string();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let npub = PublicKey::from_hex(&pubkey_hex)
        .expect("valid hex pubkey")
        .to_bech32()
        .expect("npub encode");
    let placeholder = identity_state::AccountSummary {
        id: pubkey_hex.clone(),
        npub: npub.clone(),
        display_name: None,
        signer_kind: "local".to_string(),
        status: "active".to_string(),
        signer_is_remote: false,
        is_active: true,
        picture_url: None,
    };
    kernel.set_accounts(vec![placeholder], Some(pubkey_hex.clone()));

    // Pre-condition: with no kind:0 cached, `accounts_enriched` returns the
    // placeholder verbatim — display_name still `None`.
    let before = kernel.accounts_enriched();
    assert_eq!(before.len(), 1);
    assert_eq!(
        before[0].display_name, None,
        "no kind:0 → display_name must stay None"
    );

    // Land a kind:0 with a real display name. The enrichment branch in
    // `accounts_enriched` populates `display_name` from the cache.
    kernel.test_profile_cache.upsert_view(
        &pubkey_hex,
        crate::substrate::ProfileView {
            event_id: "kind0-event".to_string(),
            created_at: 2_000,
            display: "Alice Smith".to_string(),
            picture_url: Some("https://example.com/pic.png".to_string()),
            nip05: String::new(),
            about: String::new(),
            lnurl: None,
            ..Default::default()
        },
    );

    let after = kernel.accounts_enriched();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].display_name.as_deref(), Some("Alice Smith"));
    assert_eq!(
        after[0].picture_url.as_deref(),
        Some("https://example.com/pic.png")
    );
}

const C13_PK: &str = "c13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13a";

/// Pins the djb2 avatar_color output for C13_PK so any algorithm drift is
/// caught immediately. Also verifies the format contract: 6 uppercase hex
/// chars, no '#' prefix (all other surfaces expect bare hex).
#[test]
fn avatar_color_djb2_pinned_vector() {
    let color = avatar_color_hex(C13_PK);
    // djb2 over last-6-bytes "3ac13a" starting at 5381, masked to 24 bits → "E886A1"
    assert_eq!(color, "E886A1", "djb2 pinned vector for C13_PK");
    // Format contract: exactly 6 chars, no '#', all uppercase hex
    assert_eq!(color.len(), 6, "avatar_color must be exactly 6 chars");
    assert!(color.chars().all(|c| c.is_ascii_hexdigit()), "must be hex");
    assert!(!color.starts_with('#'), "must NOT carry a '#' prefix");
    assert_eq!(color, color.to_uppercase(), "must be uppercase");
}
