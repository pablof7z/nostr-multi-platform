//! `ingest_contacts` (kind:3) unit tests: the follow-graph write, the
//! empty-list-clears-follows case, and the active-account-only source
//! recompile trigger.

use super::ingest_support::{make_event, p_tag, AUTHOR, FOLLOW_A, FOLLOW_B};
use super::*;

/// A kind:3 contact list with `p` tags updates the contacts-cache follow
/// graph: the followed hex pubkeys are stored under the author's key.
///
/// The author here is NOT the active account, so this isolates the
/// contacts-cache write from the active-account-only recompile trigger.
#[test]
fn ingest_contacts_with_p_tags_updates_follow_graph() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // No active account → the active-only follow-feed sync branch is skipped.
    assert!(
        kernel.active_account.is_none(),
        "precondition: no active account"
    );

    let event = make_event(
        "0000000000000000000000000000000000000000000000000000000000000004",
        AUTHOR,
        1_000,
        3,
        vec![
            p_tag(FOLLOW_A),
            p_tag(FOLLOW_B),
            // A non-hex `p` value must be filtered out by `is_hex_pubkey`.
            vec!["p".to_string(), "not-a-pubkey".to_string()],
            // A non-`p` tag must be ignored entirely.
            vec!["e".to_string(), FOLLOW_A.to_string()],
        ],
    );
    // `inject_contacts` persists the kind:3, then runs the chokepoint
    // projection. The kernel reacts to a transition ONLY for the active account.
    kernel.inject_contacts(event);

    let follows = crate::slots::latest_kind3_follows_from_arc(&kernel.store, AUTHOR)
        .expect("a kind:3 must be stored under the author pubkey");
    assert_eq!(
        follows,
        vec![FOLLOW_A.to_string(), FOLLOW_B.to_string()],
        "only well-formed hex `p`-tag values are kept, in tag order",
    );

    // Non-active author: the active-account contacts-transition signal does NOT
    // fire, so NO `FollowListChanged` trigger is enqueued (D4 — arbitrary peers'
    // kind:3 must not drive the kernel's follow-feed lifecycle) and
    // `timeline_authors` stays empty. (Pre-PR-3 the old `ingest_contacts`
    // enqueued an unconditional trigger even for non-active peers — a benign
    // over-fire that drove a no-op recompile; PR 3 tightens it to the active
    // account, matching the active-gate the effects always had.)
    assert_eq!(
        kernel.lifecycle.pending_trigger_count(),
        0,
        "a non-active author's kind:3 must NOT enqueue a source recompile trigger",
    );
    assert!(
        kernel.timeline_authors_for_test().is_empty(),
        "a non-active author's kind:3 must NOT mutate the timeline_authors projection",
    );
}

/// An empty kind:3 (no `p` tags) stores an empty follow vector, which is the
/// correct "cleared follow set" representation.
#[test]
fn ingest_contacts_empty_list_stores_empty_follow_vector() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Seed a non-empty contact list first.
    let seed = make_event(
        "0000000000000000000000000000000000000000000000000000000000000004",
        AUTHOR,
        1_000,
        3,
        vec![p_tag(FOLLOW_A), p_tag(FOLLOW_B)],
    );
    kernel.inject_contacts(seed);
    assert_eq!(
        crate::slots::latest_kind3_follows_from_arc(&kernel.store, AUTHOR).map(|f| f.len()),
        Some(2),
        "precondition: the seed contact list holds two follows",
    );

    // A newer kind:3 with no `p` tags → the author cleared their follow set.
    let cleared = make_event(
        "0000000000000000000000000000000000000000000000000000000000000005",
        AUTHOR,
        2_000,
        3,
        Vec::new(),
    );
    kernel.inject_contacts(cleared);

    // The event is PRESENT but derived empty — an empty `p`-tag set yields
    // `Some(vec![])`, NOT `None` (a cleared follow set is distinct from
    // "no kind:3 stored").
    let follows = crate::slots::latest_kind3_follows_from_arc(&kernel.store, AUTHOR)
        .expect("an empty kind:3 must still leave a stored contact-list event");
    assert!(
        follows.is_empty(),
        "an empty kind:3 must store an empty follow vector (cleared follow set), \
         got {follows:?}",
    );
}

/// When the kind:3 author IS the active account, `ingest_contacts` emits the
/// active-source recompile trigger. The reduced author-set expansion itself is
/// owned by the feed-source compiler, not by a bespoke core follow-feed path.
#[test]
fn ingest_contacts_for_active_account_enqueues_source_recompile() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(AUTHOR.to_string());

    let event = make_event(
        "0000000000000000000000000000000000000000000000000000000000000006",
        AUTHOR,
        1_000,
        3,
        vec![p_tag(FOLLOW_A), p_tag(FOLLOW_B)],
    );
    kernel.inject_contacts(event);

    assert!(
        crate::slots::latest_kind3_follows_from_arc(&kernel.store, AUTHOR)
            .expect("active kind:3 must be stored")
            .contains(&FOLLOW_A.to_string()),
        "active-account kind:3 must update the stored latest contact list",
    );
    assert_eq!(
        kernel.lifecycle.pending_trigger_count(),
        1,
        "active-account kind:3 must enqueue one source recompile trigger"
    );
    assert!(
        kernel.timeline_authors_for_test().is_empty(),
        "core must not project active follows into timeline_authors directly; \
         reduced feed sources own author-set expansion",
    );
}
