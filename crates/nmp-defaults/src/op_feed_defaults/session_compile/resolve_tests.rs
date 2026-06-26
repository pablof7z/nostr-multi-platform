//! Unit tests for the pure resolution helpers (#1740 step 3).
//!
//! Full open/close + admission behavior over a live `NmpApp` lives in the
//! `nmp-testing` perspective fixture (it needs `nmp_app_new`). These cover the
//! framework-internal, app-free pieces: the session WoT graph (reusing the
//! #1698 ranked query) and the typed acquisition shapes the interests use.

use super::wot_graph::SessionWotGraph;
use nmp_core::substrate::{EventId, KernelEvent};
use nmp_core::KernelEventObserver;
use nmp_planner::InterestScope;

const CONTACT_KIND: u32 = 3;
const SEED: &str = "5eed000000000000000000000000000000000000000000000000000000000001";
const F1: &str = "f1f1000000000000000000000000000000000000000000000000000000000001";
const F2: &str = "f2f2000000000000000000000000000000000000000000000000000000000001";
const CAND: &str = "ca11000000000000000000000000000000000000000000000000000000000001";

fn session_wot_graph() -> SessionWotGraph {
    SessionWotGraph::new(SEED.to_string(), CONTACT_KIND)
}

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

#[test]
fn session_wot_graph_ranks_second_degree_candidate() {
    let graph = session_wot_graph();
    // SEED follows F1, F2. F1 and F2 both follow CAND (a 2nd-degree candidate
    // SEED does not yet follow). CAND must be a ranked candidate.
    graph.on_kernel_event(&contacts(SEED, &[F1, F2]));
    graph.on_kernel_event(&contacts(F1, &[CAND]));
    graph.on_kernel_event(&contacts(F2, &[CAND]));
    let ranked = graph.ranked_candidates();
    assert!(ranked.contains(CAND), "2nd-degree candidate must rank");
    // SEED's own direct follows are NOT candidates (already followed).
    assert!(!ranked.contains(F1));
    assert!(!ranked.contains(F2));
}

#[test]
fn session_wot_graph_admits_only_candidates_fail_closed() {
    let graph = session_wot_graph();
    graph.on_kernel_event(&contacts(SEED, &[F1]));
    graph.on_kernel_event(&contacts(F1, &[CAND]));
    assert!(graph.admits(CAND));
    // A pubkey nobody in scope follows is NOT admitted (fail-closed).
    assert!(!graph.admits("dead000000000000000000000000000000000000000000000000000000000001"));
}

#[test]
fn session_wot_graph_ignores_non_contact_events() {
    let graph = session_wot_graph();
    let mut ev = contacts(SEED, &[F1]);
    ev.kind = 1;
    graph.on_kernel_event(&ev);
    graph.on_kernel_event(&contacts(F1, &[CAND]));
    // SEED's kind:1 was ignored → SEED has no follows in the graph → no candidates.
    assert!(graph.ranked_candidates().is_empty());
}

// ── Per-scope admission proofs (the predicate the resolver compiles) ──────
//
// These build the SAME framework projection the resolver registers and assert
// the resulting live predicate admits members and REJECTS non-members
// (fail-closed admission). Full open/close + interest lifecycle is proven in
// the `nmp-defaults` `tests/` integration suite over a live `NmpApp`.

use std::sync::{Arc, Mutex};

const ALICE: &str = "a11ce00000000000000000000000000000000000000000000000000000000001";
const MEMBER: &str = "5e1f000000000000000000000000000000000000000000000000000000000001";
const STRANGER: &str = "57a9000000000000000000000000000000000000000000000000000000000001";

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
    let follow_set = nmp_nip02::ActiveFollowSet::new(slot);
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
    // observers / reset hooks / extra acquisition (the set is static).
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
        resolved.resolver_observer_ids.is_empty() && resolved.reset_hooks.is_empty(),
        "a static author set installs no reactive observers/reset hooks"
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

#[test]
fn wot_tracks_seed_direct_follows_for_acquisition() {
    // The session WoT graph must expose the seed's DIRECT follows so the session
    // can acquire their kind:3 (needed to rank second-degree candidates).
    let graph = session_wot_graph();
    graph.on_kernel_event(&contacts(SEED, &[F1, F2]));
    let direct = graph.direct_follows();
    assert!(direct.contains(F1));
    assert!(direct.contains(F2));
    assert_eq!(direct.len(), 2);
}

// ── Referrer scope resolution (thread/referrer feeds) ─────────────────────

use super::resolve_static::resolve_referrer;

#[test]
fn referrer_scope_fails_closed_on_empty_id() {
    let kinds = std::collections::BTreeSet::from([1u32, 6u32]);
    let result = resolve_referrer("", &kinds);
    assert!(
        result.is_err(),
        "referrer scope with empty event_id must fail closed"
    );
}

#[test]
fn referrer_scope_fails_closed_on_no_kinds() {
    let kinds = std::collections::BTreeSet::new();
    let result = resolve_referrer("root123", &kinds);
    assert!(
        result.is_err(),
        "referrer scope with no acquisition kinds must fail closed"
    );
}

#[test]
fn referrer_scope_admits_root_by_id() {
    use super::resolve_static::resolve_referrer;
    let kinds = std::collections::BTreeSet::from([1u32, 6u32]);
    let resolved = resolve_referrer("root123", &kinds).expect("valid referrer scope");

    // The root event itself (by id)
    let root = KernelEvent {
        id: EventId::from("root123"),
        author: "alice".to_string(),
        kind: 1,
        created_at: 100,
        tags: Vec::new(),
        content: "root note".to_string(),
        relay_provenance: Vec::new(),
    };
    assert!(
        (resolved.admission)(&root),
        "admission must admit the root by id"
    );
}

#[test]
fn referrer_scope_opens_etag_tail_and_root_id_interests() {
    let kinds = std::collections::BTreeSet::from([1u32, 6u32]);
    let resolved = resolve_referrer("root123", &kinds).expect("valid referrer scope");

    assert_eq!(
        resolved.interests.len(),
        2,
        "thread scope opens #e reply-tail plus root-by-id acquisition"
    );
    for interest in &resolved.interests {
        assert_eq!(
            interest.scope,
            InterestScope::Global,
            "thread acquisition is Global/account-agnostic"
        );
    }
    let shapes: Vec<_> = resolved
        .interests
        .iter()
        .map(|interest| &interest.shape)
        .collect();
    assert!(
        shapes.iter().any(|shape| {
            shape
                .tags
                .get("e")
                .is_some_and(|values| values.contains("root123"))
                && shape.kinds == kinds
        }),
        "#e reply-tail interest must be fixed at open"
    );
    assert!(
        shapes
            .iter()
            .any(|shape| shape.event_ids.contains("root123") && shape.kinds == kinds),
        "root-by-id interest must be kind-gated and fixed at open"
    );

    let live = (resolved.live_shape)().expect("live pull shape");
    assert!(
        live.tags
            .get("e")
            .is_some_and(|values| values.contains("root123")),
        "load_older pulls the reply tail"
    );
    assert!(
        live.event_ids.is_empty(),
        "root-by-id replay is fixed acquisition, not the load_older tail"
    );
}

#[test]
fn referrer_scope_admits_events_referencing_root_via_etag() {
    let kinds = std::collections::BTreeSet::from([1u32, 6u32]);
    let resolved = resolve_referrer("root123", &kinds).expect("valid referrer scope");

    // A reply that references the root via #e tag
    let reply = KernelEvent {
        id: EventId::from("reply456"),
        author: "bob".to_string(),
        kind: 1,
        created_at: 101,
        tags: vec![vec!["e".to_string(), "root123".to_string()]],
        content: "a reply".to_string(),
        relay_provenance: Vec::new(),
    };
    assert!(
        (resolved.admission)(&reply),
        "admission must admit events with #e referencing the root"
    );
}

#[test]
fn referrer_scope_rejects_non_primary_kind_root_and_etag_rows() {
    let kinds = std::collections::BTreeSet::from([1u32, 6u32]);
    let resolved = resolve_referrer("root123", &kinds).expect("valid referrer scope");

    let wrong_kind_root = KernelEvent {
        id: EventId::from("root123"),
        author: "alice".to_string(),
        kind: 30_023,
        created_at: 100,
        tags: Vec::new(),
        content: "longform root".to_string(),
        relay_provenance: Vec::new(),
    };
    let wrong_kind_reply = KernelEvent {
        id: EventId::from("reply789"),
        author: "bob".to_string(),
        kind: 30_023,
        created_at: 101,
        tags: vec![vec!["e".to_string(), "root123".to_string()]],
        content: "longform reply".to_string(),
        relay_provenance: Vec::new(),
    };

    assert!(
        !(resolved.admission)(&wrong_kind_root),
        "root-by-id admission must still require the compiled kinds"
    );
    assert!(
        !(resolved.admission)(&wrong_kind_reply),
        "#e admission must reject events outside the compiled kinds"
    );
}

#[test]
fn referrer_scope_rejects_unrelated_events() {
    let kinds = std::collections::BTreeSet::from([1u32, 6u32]);
    let resolved = resolve_referrer("root123", &kinds).expect("valid referrer scope");

    // An unrelated event
    let unrelated = KernelEvent {
        id: EventId::from("other789"),
        author: "charlie".to_string(),
        kind: 1,
        created_at: 102,
        tags: vec![vec!["e".to_string(), "other_root".to_string()]],
        content: "unrelated note".to_string(),
        relay_provenance: Vec::new(),
    };
    assert!(
        !(resolved.admission)(&unrelated),
        "admission must reject events that don't reference the root"
    );
}
