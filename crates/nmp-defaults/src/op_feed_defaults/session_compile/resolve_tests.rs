//! Unit tests for the pure resolution helpers (#1740 step 3).
//!
//! Full open/close + admission behavior over a live `NmpApp` lives in the
//! `nmp-testing` perspective fixture (it needs `nmp_app_new`). These cover the
//! framework-internal, app-free pieces: the session WoT graph (reusing the
//! #1698 ranked query) and the filter-JSON builders the interests use.

use super::resolve::*;
use nmp_core::substrate::{EventId, KernelEvent};
use nmp_core::KernelEventObserver;

const SEED: &str = "5eed000000000000000000000000000000000000000000000000000000000001";
const F1: &str = "f1f1000000000000000000000000000000000000000000000000000000000001";
const F2: &str = "f2f2000000000000000000000000000000000000000000000000000000000001";
const CAND: &str = "ca11000000000000000000000000000000000000000000000000000000000001";

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
    let graph = SessionWotGraph::new(SEED.to_string());
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
    let graph = SessionWotGraph::new(SEED.to_string());
    graph.on_kernel_event(&contacts(SEED, &[F1]));
    graph.on_kernel_event(&contacts(F1, &[CAND]));
    assert!(graph.admits(CAND));
    // A pubkey nobody in scope follows is NOT admitted (fail-closed).
    assert!(!graph.admits("dead000000000000000000000000000000000000000000000000000000000001"));
}

#[test]
fn session_wot_graph_ignores_non_contact_events() {
    let graph = SessionWotGraph::new(SEED.to_string());
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

#[test]
fn wot_tracks_seed_direct_follows_for_acquisition() {
    // The session WoT graph must expose the seed's DIRECT follows so the session
    // can acquire their kind:3 (needed to rank second-degree candidates).
    let graph = SessionWotGraph::new(SEED.to_string());
    graph.on_kernel_event(&contacts(SEED, &[F1, F2]));
    let direct = graph.direct_follows();
    assert!(direct.contains(F1));
    assert!(direct.contains(F2));
    assert_eq!(direct.len(), 2);
}
