//! Issue #1246 — kind:3 full-edit follow-path unit tests.
//!
//! These pin the native `follow` command's contact-list edit semantics:
//!  * #1246b — fail closed when the active account's kind:3 has not been
//!    ingested yet (never rebuild from an empty list and silently wipe
//!    contacts), and
//!  * #1246a — preserve every non-`p` tag, the existing follows' relay-hint +
//!    petname columns, and the original content across an edit.
//!
//! Extracted from the sibling `tests.rs` to keep that file under the file-size
//! hard cap. As a child module of `tests`, `use super::*` inherits the shared
//! helpers (`fresh`, `sign_in_with_nip65`, `seed_contact_list`, `follow`,
//! `last_published_event_json`, `tags_of`).

use super::*;

#[test]
fn follow_publishes_kind3_with_p_tag() {
    // The active account's kind:3 must be loaded before an edit is allowed
    // (issue #1246b fail-closed gate). Seed an existing (possibly empty)
    // contact list, then add a follow.
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let author = id.active_pubkey().unwrap();
    seed_contact_list(&mut kernel, &author, &[]);
    let target = "b".repeat(64);
    let outbound = follow(
        &id,
        &mut kernel,
        &target,
        true,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );
    assert!(!outbound.is_empty());
    assert!(outbound[0].text.contains("\"kind\":3"));
    assert!(outbound[0].text.contains(&target));
}

#[test]
fn follow_fails_closed_when_kind3_not_loaded() {
    // issue #1246b: the native path must NOT rebuild a kind:3 from an empty
    // list when the active account's contact list has not been ingested yet —
    // doing so would silently wipe the user's contacts. With no kind:3 seeded,
    // `follow` must publish nothing and surface a fail-closed toast (matching
    // the wasm `follow_list_not_loaded` CapabilityFailure).
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let target = "b".repeat(64);
    let outbound = follow(
        &id,
        &mut kernel,
        &target,
        true,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );
    assert!(
        outbound.is_empty(),
        "follow with an unloaded kind:3 must publish nothing (fail closed)"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("follow_list_not_loaded")),
        "fail-closed follow must surface a follow_list_not_loaded toast"
    );
    assert!(
        kernel.publish_queue_snapshot().is_empty(),
        "fail-closed follow must not enqueue a publish"
    );
}

#[test]
fn follow_preserves_relay_hints_petnames_and_content_on_edit() {
    // issue #1246a: editing the follow set must preserve every non-`p` tag,
    // the existing follows' relay-hint + petname columns, and the original
    // content. Seed a rich kind:3, add a new follow, and assert nothing else
    // changed in the re-published event.
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let author = id.active_pubkey().unwrap();
    let kept = "c".repeat(64);
    let relay = "wss://hint.example";
    let petname = "carol";
    kernel.inject_replaceable_event(
        &"3".repeat(64),
        &author,
        1_700_000_000,
        3,
        vec![
            vec!["r".to_string(), relay.to_string(), "read".to_string()],
            vec![
                "p".to_string(),
                kept.clone(),
                relay.to_string(),
                petname.to_string(),
            ],
        ],
        "wss://seed-relay.test",
        1,
    );

    let target = "b".repeat(64);
    let outbound = follow(
        &id,
        &mut kernel,
        &target,
        true,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );
    assert!(!outbound.is_empty(), "follow must re-publish the kind:3");
    let event = last_published_event_json(&outbound);
    let tags = tags_of(&event);

    // Non-`p` tag preserved verbatim.
    assert!(
        tags.contains(&vec![
            "r".to_string(),
            relay.to_string(),
            "read".to_string()
        ]),
        "non-`p` tag must survive the edit"
    );
    // Existing follow keeps relay hint + petname.
    assert!(
        tags.contains(&vec![
            "p".to_string(),
            kept.clone(),
            relay.to_string(),
            petname.to_string(),
        ]),
        "existing follow's relay hint + petname must survive the edit"
    );
    // New follow appended.
    assert!(
        tags.contains(&vec!["p".to_string(), target.clone()]),
        "new follow must be appended"
    );
}

#[test]
fn follow_many_creates_first_kind3_for_brand_new_account() {
    // P0-A on-device regression: a brand-new onboarding account is created with
    // EMPTY `initial_follows`, so NO cold-start kind:3 is ever published and the
    // event store has no kind:3 for it. The user then selects follow packs and
    // `follow_many` runs. It must NOT fail closed — a freshly-generated local
    // key has no REMOTE contact list to clobber, so its empty follow set is
    // authoritative — and must publish the account's FIRST kind:3 containing
    // EVERY selected pubkey.
    let (mut id, mut kernel) = fresh();
    // Production composition installs `nmp_nip01::ContactsCache`; the in-crate
    // harness defaults to `EmptyContactsLookup` (every list reports unknown), so
    // install the substrate test cache to exercise the real "contacts known"
    // signal `create_account` seeds.
    kernel.set_contacts_lookup(std::sync::Arc::new(
        crate::substrate::TestContactsCache::new(),
    ));

    let profile = std::collections::HashMap::new();
    let relays = vec![("wss://onboard.relay/".to_string(), "both".to_string())];
    create_account(
        &mut id,
        &mut kernel,
        false,
        &profile,
        &relays,
        &[],
        false,
        true,
    );
    let author = id.active_pubkey().expect("new account pubkey");
    // Guarantee the outbox resolver routes the kind:3 publish to a write relay.
    kernel.seed_kind10002_for_test(&author, &["wss://onboard.relay/"]);

    let a = "a".repeat(64);
    let b = "b".repeat(64);
    let c = "c".repeat(64);
    let outbound = follow_many(
        &id,
        &mut kernel,
        &[a.clone(), b.clone(), c.clone()],
        None,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );

    assert!(
        !outbound.is_empty(),
        "follow_many on a brand-new account must publish a kind:3, not fail closed"
    );
    assert!(
        !kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("follow_list_not_loaded")),
        "a brand-new account must NOT surface follow_list_not_loaded"
    );
    let event = last_published_event_json(&outbound);
    assert_eq!(event["kind"], 3, "follow_many must publish a kind:3");
    let p: std::collections::HashSet<String> = tags_of(&event)
        .into_iter()
        .filter(|t| t.first().map(String::as_str) == Some("p"))
        .filter_map(|t| t.get(1).cloned())
        .collect();
    assert!(
        p.contains(&a) && p.contains(&b) && p.contains(&c),
        "the account's first kind:3 must contain ALL selected pubkeys; got {p:?}"
    );
    assert_eq!(p.len(), 3, "exactly the three selected pubkeys, no more");
}

#[test]
fn follow_many_fails_closed_for_unsynced_existing_account() {
    // Safety counterpart: an EXISTING account (signed in via nsec, kind:3 NOT
    // yet synced from relays, contacts cache has no entry) must STILL fail
    // closed — publishing here would silently clobber the unsynced remote
    // contact list. This is the "list exists remotely but is not loaded" case
    // the gate must keep distinguishing from a brand-new local account.
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);

    let outbound = follow_many(
        &id,
        &mut kernel,
        &["a".repeat(64)],
        None,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );
    assert!(
        outbound.is_empty(),
        "follow_many on an unsynced existing account must publish nothing (fail closed)"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("follow_list_not_loaded")),
        "fail-closed follow_many must surface a follow_list_not_loaded toast"
    );
}
