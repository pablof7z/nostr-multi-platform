//! Per-scope admission proofs for `ListMembers`/`ContactList`/`Authors` (split
//! out of `resolve_tests.rs` for file-size discipline — pre-#3086-merge
//! polish).
//!
//! These build the SAME framework projection the resolver registers and
//! assert the resulting live predicate admits members and REJECTS
//! non-members (fail-closed admission). Full open/close + interest lifecycle
//! is proven in the `explicit composition` `tests/` integration suite over a
//! live `NmpApp`.

use nmp_core::substrate::{EventId, KernelEvent};
use nmp_core::ObservedProjectionSink;
use nmp_planner::InterestScope;
use std::sync::{Arc, Mutex};

const ALICE: &str = "a11ce00000000000000000000000000000000000000000000000000000000001";
const MEMBER: &str = "5e1f000000000000000000000000000000000000000000000000000000000001";
const STRANGER: &str = "57a9000000000000000000000000000000000000000000000000000000000001";

fn contacts(author: &str, follows: &[&str]) -> KernelEvent {
    let tags = follows
        .iter()
        .map(|pk| vec!["p".to_string(), pk.to_string()])
        .collect();
    KernelEvent {
        id: EventId::from("0".repeat(64)),
        author: author.to_string(),
        kind: 3,
        created_at: 100,
        tags,
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn list_event(author: &str, d_tag: &str, members: &[&str]) -> KernelEvent {
    let mut tags = vec![vec!["d".to_string(), d_tag.to_string()]];
    for pk in members {
        tags.push(vec!["p".to_string(), pk.to_string()]);
    }
    KernelEvent {
        id: EventId::from("1".repeat(64)),
        author: author.to_string(),
        kind: 30_000,
        created_at: 100,
        tags,
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

#[test]
fn list_members_predicate_admits_members_rejects_strangers() {
    // The ListMembers resolver builds a PeopleListProjection over the active
    // slot and a live predicate over `members(list_id)`.
    let slot = Arc::new(Mutex::new(Some(ALICE.to_string())));
    let projection = Arc::new(nmp_nip51::PeopleListProjection::new(slot));
    projection.on_kernel_event(&list_event(ALICE, "team", &[MEMBER]));

    let list_id = "team".to_string();
    let proj = Arc::clone(&projection);
    let admit = move |pk: &str| proj.members(&list_id).contains(pk);

    assert!(admit(MEMBER), "list member is admitted");
    assert!(!admit(STRANGER), "non-member is NOT admitted (fail-closed)");
}

#[test]
fn contact_list_predicate_admits_follows_rejects_strangers() {
    // The active-owner ContactList resolver builds an ActiveFollowSet and uses
    // its live predicate.
    let slot = Arc::new(Mutex::new(Some(ALICE.to_string())));
    let store_slot = nmp_core::slots::new_event_store_slot();
    let follow_set =
        nmp_nip02::ActiveFollowSet::new(slot, nmp_nip02::LatestKind3FollowSet::new(store_slot));
    // Deliver the active account's kind:3 follow list.
    follow_set.on_kernel_event(&contacts(ALICE, &[MEMBER]));
    let admit = follow_set.predicate();

    assert!(admit(MEMBER), "followed pubkey is admitted");
    assert!(!admit(STRANGER), "non-follow is NOT admitted (fail-closed)");
}

// ── Authors { authors } — static author-set timeline ─────────────────────

fn note(author: &str) -> KernelEvent {
    note_kind(author, 1)
}

fn note_kind(author: &str, kind: u32) -> KernelEvent {
    KernelEvent {
        id: EventId::from("2".repeat(64)),
        author: author.to_string(),
        kind,
        created_at: 100,
        tags: Vec::new(),
        content: "a note".to_string(),
        relay_provenance: Vec::new(),
    }
}

#[test]
fn authors_scope_admits_only_the_target_authors_rejects_others() {
    // The load-bearing proof: an author feed admits ONLY events authored BY the
    // target set — NOT a stranger's, NOT (because this is the author's OWN
    // timeline, not their follows) anyone else's.
    let authors: std::collections::BTreeSet<String> = [ALICE.to_string(), MEMBER.to_string()]
        .into_iter()
        .collect();
    let kinds: std::collections::BTreeSet<u32> = [1u32].into_iter().collect();
    let resolved = super::resolve_static::resolve_authors(&authors, &kinds)
        .expect("non-empty author set resolves");

    // Admission is EVENT-AWARE over the author set.
    assert!(
        (resolved.admission)(&note(ALICE)),
        "a target author's note is admitted"
    );
    assert!(
        (resolved.admission)(&note(MEMBER)),
        "every target author's note is admitted"
    );
    assert!(
        !(resolved.admission)(&note_kind(ALICE, 30_023)),
        "a target author's non-primary kind is excluded"
    );
    assert!(
        !(resolved.admission)(&note(STRANGER)),
        "a NON-author's note is excluded (the proof — author scope is not 'admit any')"
    );

    // Acquisition: ONE fixed author+kind interest, Global scope. No reactive
    // observers / hooks / extra acquisition (the set is static).
    assert_eq!(
        resolved.interests.len(),
        1,
        "one fixed acquisition interest"
    );
    let interest = &resolved.interests[0];
    assert_eq!(
        interest.scope,
        InterestScope::Global,
        "Global scope (account-agnostic author pin)"
    );
    let shape = &interest.shape;
    assert_eq!(
        shape.authors, authors,
        "acquires exactly the target authors"
    );
    assert_eq!(shape.kinds, kinds, "acquires exactly the compiled kinds");
    assert!(
        resolved.resolver_observer_ids.is_empty() && resolved.reactivity_hooks.is_empty(),
        "a static author set installs no reactive observers/hooks"
    );

    // The live (pull-pager) shape mirrors the fixed acquisition.
    let live = (resolved.live_shape)().expect("static shape is always present");
    assert_eq!(live.authors, authors);
    assert_eq!(live.kinds, kinds);
}

#[test]
fn authors_scope_fails_closed_on_empty_set_or_no_kinds() {
    let kinds: std::collections::BTreeSet<u32> = [1u32].into_iter().collect();
    // Empty author set → fail closed (never "admit everyone"): no interest opened.
    let empty: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    assert!(
        super::resolve_static::resolve_authors(&empty, &kinds).is_err(),
        "an empty author set must fail closed, not resolve to admit-any"
    );
    // No acquisition kinds → fail closed (nothing to acquire).
    let authors: std::collections::BTreeSet<String> = [ALICE.to_string()].into_iter().collect();
    assert!(
        super::resolve_static::resolve_authors(&authors, &std::collections::BTreeSet::new())
            .is_err(),
        "no acquisition kinds must fail closed"
    );
}
