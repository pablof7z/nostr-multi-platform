use super::*;

fn author(n: u16) -> String {
    format!("{n:064x}")
}

fn p(pubkey: &str) -> Vec<String> {
    vec!["p".to_string(), pubkey.to_string()]
}

#[test]
fn direct_follows_beat_second_degree() {
    let me = author(1);
    let direct = author(2);
    let indirect = author(3);

    let mut graph = WotGraph::default();
    graph.ingest_follow_list(&me, &[p(&direct)]);
    graph.ingest_follow_list(&direct, &[p(&indirect)]);

    assert_eq!(graph.score(&me, &direct).score, DIRECT_FOLLOW_SCORE);
    assert_eq!(graph.score(&me, &indirect).score, SECOND_DEGREE_SCORE);
}

#[test]
fn many_followed_mutes_hide_unfollowed_candidate() {
    let me = author(1);
    let candidate = author(9);
    let alice = author(2);
    let bob = author(3);

    let mut graph = WotGraph::default();
    graph.ingest_follow_list(&me, &[p(&alice), p(&bob)]);
    graph.ingest_mute_list(&alice, &[p(&candidate)]);
    graph.ingest_mute_list(&bob, &[p(&candidate)]);

    let decision = graph.score(&me, &candidate);
    assert_eq!(decision.score, DEFAULT_AUTO_HIDE_SCORE);
    assert!(decision.hide);
    assert_eq!(decision.reason, "muted-by-followed");
}

#[test]
fn apps_build_presets_from_public_policy_constants() {
    // An app crate should be able to express "default or stricter" trust presets
    // by referencing NMP's authoritative tier constants instead of cloning the
    // magic numbers locally (issue #1623). A "close-friends" preset that admits
    // only direct follows is just `DIRECT_FOLLOW_SCORE` as the minimum floor.
    let me = author(1);
    let direct = author(2);
    let mutual = author(3);
    let second_degree = author(4);

    let mut graph = WotGraph::default();
    graph.ingest_follow_list(&me, &[p(&direct), p(&mutual)]);
    graph.ingest_follow_list(&mutual, &[p(&second_degree)]);

    // Default policy keeps the second-degree candidate visible.
    assert!(!graph.score(&me, &second_degree).hide);

    // A stricter, app-owned preset built from the public constant hides anyone
    // below direct-follow strength — no duplicated policy numbers required.
    let strict = graph.batch_score_with_minimum_score(
        &me,
        [direct.as_str(), second_degree.as_str()],
        DIRECT_FOLLOW_SCORE,
    );
    assert!(
        !strict[0].hide,
        "direct follow passes the close-friends floor"
    );
    assert!(
        strict[1].hide,
        "second-degree is hidden by the stricter preset"
    );
}

#[test]
fn self_mute_overrides_everything() {
    let me = author(1);
    let candidate = author(2);

    let mut graph = WotGraph::default();
    graph.ingest_follow_list(&me, &[p(&candidate)]);
    graph.ingest_mute_list(&me, &[p(&candidate)]);

    assert_eq!(graph.score(&me, &candidate).reason, "muted-by-self");
}

#[test]
fn configurable_threshold_hides_unknown_without_hiding_direct_follow() {
    let me = author(1);
    let direct = author(2);
    let unknown = author(3);

    let mut graph = WotGraph::default();
    graph.ingest_follow_list(&me, &[p(&direct)]);

    let direct_decision = graph.score_with_minimum_score(&me, &direct, 10);
    assert_eq!(direct_decision.score, DIRECT_FOLLOW_SCORE);
    assert!(!direct_decision.hide);

    let unknown_decision = graph.score_with_minimum_score(&me, &unknown, 10);
    assert_eq!(unknown_decision.score, 0);
    assert!(unknown_decision.hide);
    assert_eq!(unknown_decision.reason, "unknown");
}

#[test]
fn batch_score_preserves_candidate_order() {
    let me = author(1);
    let direct = author(2);
    let mutual = author(3);
    let candidate = author(4);
    let unknown = author(99);

    let mut graph = WotGraph::default();
    graph.ingest_follow_list(&me, &[p(&direct), p(&mutual)]);
    graph.ingest_follow_list(&mutual, &[p(&candidate)]);

    let decisions = graph.batch_score(&me, [candidate.as_str(), direct.as_str(), unknown.as_str()]);

    assert_eq!(decisions[0].reason, "second-degree");
    assert_eq!(decisions[1].reason, "direct-follow");
    assert_eq!(decisions[2].reason, "unknown");
}

#[test]
fn mutual_follows_and_stats_are_deterministic() {
    let me = author(1);
    let alice = author(2);
    let bob = author(3);
    let candidate = author(4);

    let mut graph = WotGraph::default();
    graph.ingest_follow_list(&me, &[p(&bob), p(&alice)]);
    graph.ingest_follow_list(&bob, &[p(&candidate)]);
    graph.ingest_follow_list(&alice, &[p(&candidate)]);
    graph.ingest_mute_list(&me, &[p(&author(9))]);

    assert_eq!(graph.mutual_follows(&me, &candidate), vec![alice, bob]);
    assert!(graph.directly_follows(&me, &author(2)));
    assert_eq!(
        graph.stats(),
        WotGraphStats {
            follow_authors: 3,
            mute_authors: 1,
        }
    );
}
