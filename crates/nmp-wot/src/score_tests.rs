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
fn cold_viewer_with_fallback_root_scores_as_if_rooted_at_fallback() {
    let cold_viewer = author(1);
    let seed_root = author(2);
    let candidate = author(3);

    let mut graph = WotGraph::default();
    // `cold_viewer` never ingested a kind:3 at all. `seed_root` is a
    // caller-owned bootstrap trust seed that directly follows `candidate`.
    graph.ingest_follow_list(&seed_root, &[p(&candidate)]);

    assert!(!graph.has_follows(&cold_viewer));

    let decision =
        graph.score_rooted(&cold_viewer, Some(seed_root.as_str()), &candidate);
    assert_eq!(decision.score, DIRECT_FOLLOW_SCORE);
    assert_eq!(decision.reason, "direct-follow");
    assert!(!decision.hide);
    assert!(decision.rooted_at_fallback);

    let floored = graph.score_rooted_with_minimum_score(
        &cold_viewer,
        Some(seed_root.as_str()),
        &candidate,
        10,
    );
    assert_eq!(floored.score, DIRECT_FOLLOW_SCORE);
    assert!(!floored.hide);
    assert!(floored.rooted_at_fallback);

    let batch = graph.batch_score_rooted(
        &cold_viewer,
        Some(seed_root.as_str()),
        [candidate.as_str()],
    );
    assert_eq!(batch[0].score, DIRECT_FOLLOW_SCORE);
    assert!(batch[0].rooted_at_fallback);
}

#[test]
fn viewer_with_follows_ignores_fallback_root() {
    let me = author(1);
    let seed_root = author(2);
    let candidate = author(3);

    let mut graph = WotGraph::default();
    // `me` has an established (if small) follow graph of my own, so the
    // fallback root must never be consulted even though it also has an
    // opinion about `candidate`.
    graph.ingest_follow_list(&me, &[p(&author(42))]);
    graph.ingest_follow_list(&seed_root, &[p(&candidate)]);

    assert!(graph.has_follows(&me));

    let decision = graph.score_rooted(&me, Some(seed_root.as_str()), &candidate);
    // `me` does not follow `candidate` directly or through a second-degree
    // edge, so this must be "unknown" from `me`'s own graph — not the
    // direct-follow score the fallback root would have produced.
    assert_eq!(decision.score, 0);
    assert_eq!(decision.reason, "unknown");
    assert!(!decision.rooted_at_fallback);
}

#[test]
fn cold_viewer_without_fallback_root_matches_todays_behavior() {
    let cold_viewer = author(1);
    let candidate = author(2);

    let graph = WotGraph::default();
    assert!(!graph.has_follows(&cold_viewer));

    let default_decision = graph.score(&cold_viewer, &candidate);
    let rooted_decision = graph.score_rooted(&cold_viewer, None, &candidate);
    assert_eq!(rooted_decision.score, default_decision.score);
    assert_eq!(rooted_decision.hide, default_decision.hide);
    assert_eq!(rooted_decision.reason, default_decision.reason);
    assert_eq!(rooted_decision.score, 0);
    // 0 is above the default auto-hide floor (-50): today's default policy
    // does not hide an unknown candidate, it just doesn't rank it highly.
    assert!(!rooted_decision.hide);
    assert!(!rooted_decision.rooted_at_fallback);
    assert!(!default_decision.rooted_at_fallback);

    // The configurable-floor variant matches today's `score_with_minimum_score`
    // exactly too: an unknown candidate is hidden once the floor exceeds 0.
    let default_floored = graph.score_with_minimum_score(&cold_viewer, &candidate, 10);
    let rooted_floored =
        graph.score_rooted_with_minimum_score(&cold_viewer, None, &candidate, 10);
    assert_eq!(rooted_floored.score, default_floored.score);
    assert_eq!(rooted_floored.hide, default_floored.hide);
    assert!(rooted_floored.hide);
    assert!(!rooted_floored.rooted_at_fallback);
}

#[test]
fn rooted_at_fallback_flag_is_false_for_plain_score_and_score_with_minimum_score() {
    let me = author(1);
    let candidate = author(2);

    let mut graph = WotGraph::default();
    graph.ingest_follow_list(&me, &[p(&candidate)]);

    assert!(!graph.score(&me, &candidate).rooted_at_fallback);
    assert!(!graph
        .score_with_minimum_score(&me, &candidate, 0)
        .rooted_at_fallback);
    for decision in graph.batch_score(&me, [candidate.as_str()]) {
        assert!(!decision.rooted_at_fallback);
    }
    for decision in graph.batch_score_with_minimum_score(&me, [candidate.as_str()], 0) {
        assert!(!decision.rooted_at_fallback);
    }
}

#[test]
fn muted_candidates_still_hide_regardless_of_root() {
    let cold_viewer = author(1);
    let seed_root = author(2);
    let candidate = author(3);

    let mut graph = WotGraph::default();
    // The fallback root directly follows the candidate but also muted them
    // (e.g. later soured on them) — self-mute must still win even though
    // scoring is rerouted through the fallback root, not the real viewer.
    graph.ingest_follow_list(&seed_root, &[p(&candidate)]);
    graph.ingest_mute_list(&seed_root, &[p(&candidate)]);

    let decision =
        graph.score_rooted(&cold_viewer, Some(seed_root.as_str()), &candidate);
    assert_eq!(decision.reason, "muted-by-self");
    assert!(decision.hide);
    assert!(decision.rooted_at_fallback);

    // A very permissive floor still can't override a self-mute.
    let floored = graph.score_rooted_with_minimum_score(
        &cold_viewer,
        Some(seed_root.as_str()),
        &candidate,
        i32::MIN,
    );
    assert!(floored.hide);
    assert_eq!(floored.reason, "muted-by-self");
}

#[test]
fn has_follows_is_false_for_absent_and_empty_contact_lists() {
    let absent = author(1);
    let empty = author(2);
    let populated = author(3);

    let mut graph = WotGraph::default();
    graph.ingest_follow_list(&empty, &[]);
    graph.ingest_follow_list(&populated, &[p(&author(9))]);

    assert!(!graph.has_follows(&absent), "never-ingested viewer is cold");
    assert!(
        !graph.has_follows(&empty),
        "ingested-but-empty kind:3 is still cold"
    );
    assert!(graph.has_follows(&populated));
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
