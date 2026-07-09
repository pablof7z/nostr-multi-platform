//! Unit tests for the session WoT graph (#1740 step 3; split out of
//! `resolve_tests.rs` for file-size discipline — pre-#3086-merge polish).
//!
//! Full open/close + admission behavior lives in runtime/browser fixtures.
//! These cover the framework-internal, host-free WoT ranking/admission piece
//! (reusing the #1698 ranked query).

use super::wot_graph::SessionWotGraph;
use nmp_core::substrate::{EventId, KernelEvent};
use nmp_core::ObservedProjectionSink;

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
