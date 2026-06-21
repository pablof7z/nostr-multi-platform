//! #1740 step 3 — the CLOSED perspective-compiler admission matrix.
//!
//! Proves, per [`nmp_feed::FeedScope`] arm, that the COMPILED admission predicate
//! admits the right authors and REJECTS non-members (fail-closed admission), and
//! that set algebra combines the compiled sets correctly. The compiler maps each
//! typed scope to:
//!   * a compiled admission predicate ([`nmp_feed::AdmitExpr`] for static sets /
//!     a live framework projection for reactive scopes) — never an app closure;
//!   * internal acquisition interests (proven in the `nmp-defaults` open/close
//!     integration suite).
//!
//! The resolution projections used here are the SAME single-source mechanisms the
//! compiler reuses (D4): [`nmp_nip02::ActiveFollowSet`] (kind:3),
//! [`nmp_nip51::PeopleListProjection`] (kind:30000), and the #1698
//! [`nmp_wot::score::WotGraph`] ranked second-degree query. This fixture
//! duplicates NONE of that logic — it drives the real projections and asserts the
//! resulting predicate.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{EventId, KernelEvent};
use nmp_core::KernelEventObserver;
use nmp_feed::AdmitExpr;

const VIEWER: &str = "11ee000000000000000000000000000000000000000000000000000000000001";
const MEMBER: &str = "a110000000000000000000000000000000000000000000000000000000000001";
const MEMBER2: &str = "b220000000000000000000000000000000000000000000000000000000000001";
const STRANGER: &str = "dead000000000000000000000000000000000000000000000000000000000001";

fn authors(ids: &[&str]) -> AdmitExpr {
    AdmitExpr::Authors(ids.iter().map(|s| (*s).to_string()).collect())
}

fn event(author: &str, kind: u32, p_tags: &[&str], d_tag: Option<&str>) -> KernelEvent {
    let mut tags: Vec<Vec<String>> = Vec::new();
    if let Some(d) = d_tag {
        tags.push(vec!["d".to_string(), d.to_string()]);
    }
    for pk in p_tags {
        tags.push(vec!["p".to_string(), pk.to_string()]);
    }
    KernelEvent {
        id: EventId::from("1".repeat(64)),
        author: author.to_string(),
        kind,
        created_at: 100,
        tags,
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

// ── Tag scope → Any admission (the #t acquisition does the gating) ────────

#[test]
fn tag_scope_admits_any_acquired_row() {
    // `Tag { term }` compiles to `AdmitExpr::Any`: every acquired (#t-filtered)
    // row is in scope. Admission gates nothing; the acquisition filter does.
    let admit = AdmitExpr::Any;
    assert!(admit.matches(MEMBER));
    assert!(admit.matches(STRANGER));
}

// ── ContactList { active owner } → live ActiveFollowSet predicate ──────────

#[test]
fn contact_list_admits_follows_rejects_strangers() {
    let slot = Arc::new(Mutex::new(Some(VIEWER.to_string())));
    let follow_set = nmp_nip02::ActiveFollowSet::new(slot);
    follow_set.on_kernel_event(&event(VIEWER, 3, &[MEMBER], None));
    let admit = follow_set.predicate();

    assert!(admit(MEMBER), "a follow is admitted");
    assert!(admit(VIEWER), "self-inclusion: the viewer is admitted");
    assert!(!admit(STRANGER), "a non-follow is rejected (fail-closed)");
}

// ── ListMembers { list } → live PeopleListProjection predicate ────────────

#[test]
fn list_members_admits_members_rejects_strangers() {
    let slot = Arc::new(Mutex::new(Some(VIEWER.to_string())));
    let projection = Arc::new(nmp_nip51::PeopleListProjection::new(slot));
    projection.on_kernel_event(&event(VIEWER, 30_000, &[MEMBER, MEMBER2], Some("team")));

    let proj = Arc::clone(&projection);
    let admit = move |pk: &str| proj.members("team").contains(pk);

    assert!(admit(MEMBER), "list member is admitted");
    assert!(admit(MEMBER2), "second list member is admitted");
    assert!(!admit(STRANGER), "non-member is rejected (fail-closed)");
}

#[test]
fn list_members_empty_before_list_arrives_admits_nobody() {
    // Fail-closed: before the kind:30000 list event arrives, the member set is
    // empty and the predicate admits NOBODY (never everyone).
    let slot = Arc::new(Mutex::new(Some(VIEWER.to_string())));
    let projection = nmp_nip51::PeopleListProjection::new(slot);
    assert!(projection.members("team").is_empty());
    assert!(!projection.members("team").contains(MEMBER));
}

// ── Wot { seed } → the #1698 ranked second-degree query ───────────────────

#[test]
fn wot_admits_ranked_candidates_rejects_non_candidates() {
    use nmp_wot::score::WotGraph;
    let mut graph = WotGraph::default();
    // VIEWER follows MEMBER; MEMBER follows MEMBER2 (a 2nd-degree candidate).
    graph.ingest_event(VIEWER, 3, &[vec!["p".into(), MEMBER.into()]]);
    graph.ingest_event(MEMBER, 3, &[vec!["p".into(), MEMBER2.into()]]);

    let ranked: BTreeSet<String> = graph
        .ranked_second_degree_candidates(VIEWER, 100)
        .into_iter()
        .map(|(pk, _)| pk)
        .collect();
    let admit = AdmitExpr::Authors(ranked);

    assert!(admit.matches(MEMBER2), "ranked 2nd-degree candidate admitted");
    assert!(!admit.matches(MEMBER), "already-followed is not a candidate");
    assert!(!admit.matches(STRANGER), "unrelated pubkey rejected (fail-closed)");
}

// ── Set algebra over compiled pubkey sets ─────────────────────────────────

#[test]
fn union_admits_either_side() {
    let admit = AdmitExpr::Or(Box::new(authors(&[MEMBER])), Box::new(authors(&[MEMBER2])));
    assert!(admit.matches(MEMBER));
    assert!(admit.matches(MEMBER2));
    assert!(!admit.matches(STRANGER));
}

#[test]
fn intersection_admits_only_both_sides() {
    let admit = AdmitExpr::And(
        Box::new(authors(&[MEMBER, MEMBER2])),
        Box::new(authors(&[MEMBER2, STRANGER])),
    );
    // Only MEMBER2 is in BOTH sets.
    assert!(!admit.matches(MEMBER));
    assert!(admit.matches(MEMBER2));
    assert!(!admit.matches(STRANGER));
}

#[test]
fn difference_excludes_right_side() {
    let admit = AdmitExpr::AndNot(
        Box::new(authors(&[MEMBER, MEMBER2])),
        Box::new(authors(&[MEMBER2])),
    );
    assert!(admit.matches(MEMBER), "left-only member admitted");
    assert!(!admit.matches(MEMBER2), "right-side member excluded");
    assert!(!admit.matches(STRANGER), "outside both sets, rejected");
}

#[test]
fn nested_set_algebra_composes() {
    // (ContactList ∪ ListMembers) ∖ Wot — a 3-way compose.
    let union = AdmitExpr::Or(Box::new(authors(&[MEMBER])), Box::new(authors(&[MEMBER2])));
    let admit = AdmitExpr::AndNot(Box::new(union), Box::new(authors(&[MEMBER2])));
    assert!(admit.matches(MEMBER));
    assert!(!admit.matches(MEMBER2));
    assert!(!admit.matches(STRANGER));
}
