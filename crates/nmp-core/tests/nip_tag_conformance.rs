//! NIP golden-tag conformance suite.
//!
//! A table of assertions that every event kind NMP *emits* carries exactly the
//! tags its NIP mandates — and no surprising tags besides. This pins, in one
//! place, the contract that the NIP-25 `p`-tag bug (reactions missing the
//! reacted-to author) violated unnoticed despite 450+ unit tests: the bug was
//! found by inspection, not by a test. A conformance table is that test.
//!
//! ## What this suite asserts, per emitted kind
//!
//! | Kind  | NIP     | Required tags                                          |
//! |-------|---------|--------------------------------------------------------|
//! | 1     | NIP-01  | top-level note: NO `e`/`p` tags                        |
//! | 1     | NIP-10  | reply: `e`(root) + `e`(reply) markers, `p`(parent)     |
//! | 7     | NIP-25  | `e`(reacted event) + `p`(reacted author)               |
//! | 3     | NIP-02  | one `p` per followed pubkey, nothing else              |
//! | 0     | NIP-01  | metadata: NO tags (content is JSON)                    |
//! | 23194 | NIP-47  | `p`(wallet pubkey)                                     |
//! | 10002 | NIP-65  | `r` per relay, optional `read`/`write` marker          |
//!
//! ## Robustness
//!
//! Tag arrays may appear in any order on the wire. Every assertion here checks
//! tags **by key**, never by position — `tags_with_key`, `p_values`,
//! `assert_only_keys`. The one ordering-sensitive property NIP-10 actually
//! mandates (root vs. reply `e` markers) is checked via the marker column, not
//! the array index.
//!
//! ## Driving the commands
//!
//! These tests reach the (crate-private) command handlers through the
//! `test-support` facade [`nmp_core::testing::ConformanceHarness`]. The target
//! only builds with `--features test-support`; verify with:
//!
//! ```text
//! cargo test -p nmp-core --features test-support --test nip_tag_conformance
//! ```

use std::collections::HashMap;

use nmp_core::testing::ConformanceHarness;
use serde_json::Value;

/// Deterministic test identity. Same fixture key the in-crate command tests use.
const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

/// NIP-65 write relays seeded for the active account so the (fail-closed)
/// outbox resolver has targets and publish commands produce outbound frames.
const WRITE_RELAYS: &[&str] = &["wss://conformance-w1.test", "wss://conformance-w2.test"];

// ── Tag inspection helpers — key-based, order-independent ───────────────────

/// The `tags` array of an EVENT JSON object, as `Vec<Vec<String>>`.
fn tags_of(event: &Value) -> Vec<Vec<String>> {
    event["tags"]
        .as_array()
        .expect("event has a `tags` array")
        .iter()
        .map(|tag| {
            tag.as_array()
                .expect("each tag is an array")
                .iter()
                .map(|col| col.as_str().expect("tag column is a string").to_string())
                .collect()
        })
        .collect()
}

/// Every tag whose first column equals `key` (e.g. all `e` tags). Order of the
/// returned tags mirrors the wire, but callers must not depend on it.
fn tags_with_key(event: &Value, key: &str) -> Vec<Vec<String>> {
    tags_of(event)
        .into_iter()
        .filter(|t| t.first().map(String::as_str) == Some(key))
        .collect()
}

/// The value column (index 1) of every tag with first column `key`.
fn values_for_key(event: &Value, key: &str) -> Vec<String> {
    tags_with_key(event, key)
        .into_iter()
        .filter_map(|t| t.get(1).cloned())
        .collect()
}

/// The distinct set of tag keys present on the event (first column of each tag).
fn distinct_keys(event: &Value) -> Vec<String> {
    let mut keys: Vec<String> = tags_of(event)
        .into_iter()
        .filter_map(|t| t.into_iter().next())
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

/// Assert the event carries tags from exactly `allowed` keys and no others —
/// the "no forbidden or surprising tags snuck in" half of conformance.
fn assert_only_keys(event: &Value, allowed: &[&str], context: &str) {
    let mut expected: Vec<String> = allowed.iter().map(|s| s.to_string()).collect();
    expected.sort();
    expected.dedup();
    assert_eq!(
        distinct_keys(event),
        expected,
        "{context}: event carries an unexpected tag key (or is missing one)"
    );
}

/// A 64-char hex pubkey/event-id literal built from one repeated nibble.
fn hex64(nibble: char) -> String {
    std::iter::repeat_n(nibble, 64).collect()
}

/// True if `s` is a 64-char lowercase-hex string (event-id / pubkey shape).
fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn signed_harness() -> ConformanceHarness {
    let mut h = ConformanceHarness::new();
    h.sign_in_and_seed_nip65(TEST_NSEC, WRITE_RELAYS);
    h
}

// ── Kind 1 — NIP-01 top-level note ──────────────────────────────────────────

/// NIP-01: a plain top-level note has NO required tags. The conformance bar is
/// the *negative*: it must not sprout `e`/`p` tags it has no reason to carry.
#[test]
fn kind1_note_carries_no_tags() {
    let mut h = signed_harness();
    let event = h.emit_note("a plain note, no thread context", None);

    assert_eq!(event["kind"], 1, "note must be kind:1");
    assert!(
        tags_of(&event).is_empty(),
        "NIP-01 top-level note must carry no tags, got: {:?}",
        tags_of(&event)
    );
}

// ── Kind 1 — NIP-10 reply ───────────────────────────────────────────────────

/// NIP-10: a reply to a thread root must carry both an `e` "root" marker and an
/// `e` "reply" marker (marked form), plus a `p` tag re-notifying the parent's
/// author. This is the structure `nmp_nip01::Note::reply_to` emits.
#[test]
fn kind1_reply_carries_nip10_e_markers_and_parent_p_tag() {
    let mut h = signed_harness();
    let root_id = hex64('1');
    let root_author = hex64('a');
    // Seed the parent (which IS the thread root — no NIP-10 refs of its own).
    h.seed_note(&root_id, &root_author, vec![]);

    let event = h.emit_note("a reply to the root", Some(&root_id));
    assert_eq!(event["kind"], 1, "reply must be kind:1");

    // NIP-10 requires exactly one root + one reply `e` marker; here both point
    // at the parent because the parent is itself the root.
    let e_tags = tags_with_key(&event, "e");
    let root_marker = e_tags
        .iter()
        .find(|t| t.get(3).map(String::as_str) == Some("root"))
        .expect("NIP-10 reply must carry an `e` tag with a `root` marker");
    let reply_marker = e_tags
        .iter()
        .find(|t| t.get(3).map(String::as_str) == Some("reply"))
        .expect("NIP-10 reply must carry an `e` tag with a `reply` marker");
    assert_eq!(
        root_marker.get(1).map(String::as_str),
        Some(root_id.as_str()),
        "the `root` marker must reference the thread root event id"
    );
    assert_eq!(
        reply_marker.get(1).map(String::as_str),
        Some(root_id.as_str()),
        "the `reply` marker must reference the direct parent event id"
    );

    // NIP-10 §p-tags: the parent's author must be re-notified. This is the
    // exact class of tag the NIP-25 review found missing on reactions.
    let p_values = values_for_key(&event, "p");
    assert!(
        p_values.contains(&root_author),
        "NIP-10 reply must carry a `p` tag for the parent author ({root_author}), got: {p_values:?}"
    );

    // No tag keys beyond `e` and `p` on a reply.
    assert_only_keys(&event, &["e", "p"], "NIP-10 reply");
}

// ── Kind 7 — NIP-25 reaction ────────────────────────────────────────────────

/// NIP-25: a kind:7 reaction must carry an `e` tag (the reacted-to event) AND a
/// `p` tag (that event's author) so the author's relays route the reaction to
/// their notification inbox. The missing `p` tag here was the bug that
/// motivated this whole suite.
#[test]
fn kind7_reaction_carries_e_and_p_tags() {
    let mut h = signed_harness();
    let target_id = hex64('e');
    let target_author = hex64('c');
    // Seed the reacted-to event so its author is resolvable from the read-cache.
    h.seed_note(&target_id, &target_author, vec![]);

    let event = h.emit_reaction(&target_id, "+");
    assert_eq!(event["kind"], 7, "reaction must be kind:7");

    // Exactly one `e` tag → the reacted-to event.
    let e_values = values_for_key(&event, "e");
    assert_eq!(
        e_values,
        vec![target_id.clone()],
        "NIP-25 reaction must carry exactly one `e` tag for the reacted-to event"
    );

    // Exactly one `p` tag → the reacted-to event's author. The regression pin.
    let p_values = values_for_key(&event, "p");
    assert_eq!(
        p_values,
        vec![target_author.clone()],
        "NIP-25 reaction must carry a `p` tag for the reacted-to author — \
         the missing-`p` bug this suite exists to catch"
    );

    assert_only_keys(&event, &["e", "p"], "NIP-25 reaction");
}

// ── Kind 3 — NIP-02 contact list ────────────────────────────────────────────

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

// ── Kind 0 — NIP-01 metadata ────────────────────────────────────────────────

/// NIP-01: a kind:0 metadata event has NO required tags — the profile fields
/// live in the JSON `content`, not in tags. Conformance is the negative:
/// metadata must not carry tags. Driven through `publish_unsigned_event`, the
/// production path for a profile/display-name update.
#[test]
fn kind0_metadata_carries_no_tags() {
    let mut h = signed_harness();
    let event = h.emit_unsigned(
        0,
        vec![],
        r#"{"name":"marcus","display_name":"Marcus Webb"}"#,
    );

    assert_eq!(event["kind"], 0, "metadata must be kind:0");
    assert!(
        tags_of(&event).is_empty(),
        "NIP-01 kind:0 metadata must carry no tags, got: {:?}",
        tags_of(&event)
    );
    // The profile JSON rides in `content`, not in tags.
    assert!(
        event["content"]
            .as_str()
            .is_some_and(|c| c.contains("Marcus Webb")),
        "kind:0 metadata content must carry the profile JSON"
    );
}

// ── Kind 23194 — NIP-47 NWC request ─────────────────────────────────────────

/// NIP-47: a kind:23194 wallet request must carry a `p` tag naming the wallet
/// service pubkey — that is how the relay routes the encrypted request to the
/// wallet. `wallet_connect` emits a `get_info` / `get_balance` request pair;
/// every one of them is asserted.
#[cfg(feature = "wallet")]
#[test]
fn kind23194_nwc_request_carries_wallet_p_tag() {
    // A NWC URI: nostr+walletconnect://<wallet-pubkey>?relay=<wss>&secret=<hex>.
    // Both the wallet pubkey and the client secret must be real secp256k1
    // values — the request content is NIP-04-encrypted to the wallet pubkey,
    // which fails on a non-curve-point. `wallet_pubkey` here is the x-only
    // pubkey of secret `0x..01` (a valid curve point); `client_secret` is a
    // distinct valid secret key.
    let wallet_pubkey = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
    let client_secret = "0000000000000000000000000000000000000000000000000000000000000002";
    let uri = format!(
        "nostr+walletconnect://{wallet_pubkey}?relay=wss://nwc-relay.test&secret={client_secret}"
    );

    let mut h = ConformanceHarness::new();
    let requests = h.emit_wallet_connect(&uri);
    assert!(
        !requests.is_empty(),
        "wallet_connect must emit at least one kind:23194 request"
    );

    for request in &requests {
        assert_eq!(request["kind"], 23194, "NWC request must be kind:23194");
        let p_values = values_for_key(request, "p");
        assert_eq!(
            p_values,
            vec![wallet_pubkey.to_string()],
            "NIP-47 kind:23194 must carry exactly one `p` tag naming the wallet pubkey"
        );
        // A NWC request carries the wallet `p` tag and nothing else.
        assert_only_keys(request, &["p"], "NIP-47 NWC request");
    }
}

// ── Kind 10002 — NIP-65 relay list ──────────────────────────────────────────

/// NIP-65 `r`-tag structural check, reused by both kind:10002 tests below.
/// Asserts: one `r` tag per relay, every declared URL present, and any marker
/// column limited to `read`/`write` — and no tag keys besides `r`.
fn assert_nip65_relay_list(event: &Value, expected_urls: &[&str]) {
    assert_eq!(event["kind"], 10002, "relay list must be kind:10002");
    let r_tags = tags_with_key(event, "r");
    assert_eq!(
        r_tags.len(),
        expected_urls.len(),
        "NIP-65 kind:10002 must carry exactly one `r` tag per relay"
    );
    let r_urls = values_for_key(event, "r");
    for url in expected_urls {
        assert!(
            r_urls.contains(&url.to_string()),
            "NIP-65 kind:10002 must carry an `r` tag for relay {url}"
        );
    }
    for tag in &r_tags {
        if let Some(marker) = tag.get(2) {
            assert!(
                marker == "read" || marker == "write",
                "NIP-65 `r` marker must be `read` or `write`, got: {marker:?}"
            );
        }
    }
    assert_only_keys(event, &["r"], "NIP-65 relay list");
}

/// NIP-65: when NMP publishes a kind:10002 relay list, it must carry one `r`
/// tag per relay (optionally `read`/`write`-marked) and nothing else.
///
/// This drives the generic publish path (`publish_unsigned_event`) with a
/// kind:10002 carrying NIP-65 `r` tags — the same kind, tags and signing path
/// `create_account`'s relay-list builder feeds into. It pins that NMP's
/// publish pipeline emits a kind:10002 with its `r`-tag structure intact.
#[test]
fn kind10002_relay_list_carries_r_tags() {
    let mut h = signed_harness();
    let r_tags = vec![
        vec!["r".to_string(), "wss://nip65-write.test".to_string(), "write".to_string()],
        vec!["r".to_string(), "wss://nip65-read.test".to_string(), "read".to_string()],
        // An unmarked `r` tag — NIP-65 reads this as both read and write.
        vec!["r".to_string(), "wss://nip65-both.test".to_string()],
    ];
    let event = h.emit_unsigned(10002, r_tags, "");
    assert_nip65_relay_list(
        &event,
        &[
            "wss://nip65-write.test",
            "wss://nip65-read.test",
            "wss://nip65-both.test",
        ],
    );
}

/// Conformance FINDING — documented gap, not a passing-by-fiction row.
///
/// `create_account` (`identity.rs`) is the only production code that builds a
/// kind:10002 from user-supplied relays. It hands the event to
/// `publish_signed`, but a freshly-created account has no NIP-65 outbox of its
/// own yet, so the fail-closed `Nip65OutboxResolver` returns `NoTargets` and
/// the publish engine drops the event without persisting it. The kind:10002 is
/// therefore **built but never emitted to the wire** on account creation.
///
/// This test pins that finding with a real assertion: `create_account`
/// produces no observable kind:10002. If a future change makes it observable
/// (self-ingest of the just-published relay list, or seeding the bootstrap
/// relays into the outbox), this assertion fails loudly — the signal to
/// promote the kind:10002 row to a full `assert_nip65_relay_list` check.
#[test]
fn finding_create_account_kind10002_built_but_not_emitted() {
    let mut h = ConformanceHarness::new();
    let relays: Vec<(String, String)> = vec![
        ("wss://nip65-write.test".to_string(), "write".to_string()),
        ("wss://nip65-read.test".to_string(), "read".to_string()),
    ];
    let mut profile = HashMap::new();
    profile.insert("display_name".to_string(), "Marcus Webb".to_string());
    h.create_account(profile, &relays);

    assert!(
        h.published_event_of_kind(10002).is_none(),
        "FINDING REGRESSED (good news): create_account now emits an observable \
         kind:10002 — promote this to assert_nip65_relay_list and delete this \
         documented-gap test"
    );
}

// ── Cross-cutting: no command leaks an `e`/`p` tag where the NIP forbids it ──

/// A non-reply note and a kind:0 metadata event are the two "tagless" emit
/// paths. A regression that started attaching thread/notification tags to
/// either would be a conformance break — pin both in one place.
#[test]
fn tagless_kinds_never_emit_e_or_p_tags() {
    let mut h = signed_harness();

    let note = h.emit_note("tagless note", None);
    assert!(
        tags_with_key(&note, "e").is_empty() && tags_with_key(&note, "p").is_empty(),
        "a top-level kind:1 note must never emit `e` or `p` tags"
    );

    let metadata = h.emit_unsigned(0, vec![], r#"{"display_name":"Nobody"}"#);
    assert!(
        tags_with_key(&metadata, "e").is_empty() && tags_with_key(&metadata, "p").is_empty(),
        "a kind:0 metadata event must never emit `e` or `p` tags"
    );
}
