use nmp_wot::{SignalGraph, SignalIngest, TrustConfig, TrustDecision};

#[derive(Clone, Debug, Eq, PartialEq)]
struct Note {
    author: String,
    body: &'static str,
}

fn pk(n: u8) -> String {
    format!("{n:064x}")
}

fn p_tags(pubkeys: &[String]) -> Vec<Vec<String>> {
    pubkeys
        .iter()
        .map(|pubkey| vec!["p".to_string(), pubkey.clone()])
        .collect()
}

#[test]
fn event_ingest_accepts_follow_and_public_mute_p_tags() {
    let viewer = pk(1);
    let followed = pk(2);
    let muted = pk(3);
    let mut graph = SignalGraph::new();

    let follow_ingest = graph.ingest_event_tags(
        nmp_wot::KIND_CONTACT_LIST,
        &viewer,
        &[
            vec!["p".to_string(), followed.clone()],
            vec!["p".to_string(), "not-a-pubkey".to_string()],
            vec!["e".to_string(), pk(4)],
        ],
    );
    assert_eq!(
        follow_ingest,
        SignalIngest::FollowList {
            author: viewer.clone(),
            follows: 1
        }
    );

    let mute_ingest =
        graph.ingest_event_tags(nmp_wot::KIND_MUTE_LIST, &viewer, &p_tags(&[muted.clone()]));
    assert_eq!(
        mute_ingest,
        SignalIngest::MuteList {
            author: viewer.clone(),
            mutes: 1
        }
    );

    assert!(graph.directly_follows(&viewer, &followed));
    assert!(graph.directly_mutes(&viewer, &muted));
    assert_eq!(graph.stats().follow_edges, 1);
    assert_eq!(graph.stats().mute_edges, 1);
}

#[test]
fn direct_follows_are_closest_and_rank_before_second_hop_and_unknown() {
    let viewer = pk(1);
    let direct = pk(2);
    let second_hop = pk(3);
    let unknown = pk(4);
    let mut graph = SignalGraph::new();
    graph.upsert_follow_list(viewer.clone(), [direct.clone()]);
    graph.upsert_follow_list(direct.clone(), [second_hop.clone()]);

    let trust = graph.compute_trust(&viewer, TrustConfig::default());
    assert_eq!(trust.score_for(&direct).distance, Some(1));
    assert_eq!(trust.score_for(&second_hop).distance, Some(2));
    assert_eq!(trust.score_for(&unknown).distance, None);

    let ranked = trust.rank_pubkeys([unknown.clone(), second_hop.clone(), direct.clone()]);
    let ranked_pubkeys = ranked
        .into_iter()
        .map(|entry| entry.pubkey)
        .collect::<Vec<_>>();
    assert_eq!(ranked_pubkeys, vec![direct, second_hop, unknown]);
}

#[test]
fn trusted_community_mutes_hide_unfollowed_authors() {
    let viewer = pk(1);
    let trusted_a = pk(2);
    let trusted_b = pk(3);
    let noisy = pk(4);
    let mut graph = SignalGraph::new();
    graph.upsert_follow_list(viewer.clone(), [trusted_a.clone(), trusted_b.clone()]);
    graph.upsert_mute_list(trusted_a, [noisy.clone()]);
    graph.upsert_mute_list(trusted_b, [noisy.clone()]);

    let trust = graph.compute_trust(&viewer, TrustConfig::default());
    let score = trust.score_for(&noisy);

    assert_eq!(score.muted_by_viewer, false);
    assert_eq!(score.followed_by_viewer, false);
    assert_eq!(score.muted_by_trusted_count, 2);
    assert!(score.score < 0.0);
    assert_eq!(score.decision, TrustDecision::Hide);
    assert!(trust.should_hide(&noisy));
}

#[test]
fn direct_follow_is_not_hidden_by_community_mutes() {
    let viewer = pk(1);
    let trusted = pk(2);
    let followed_target = pk(3);
    let mut graph = SignalGraph::new();
    graph.upsert_follow_list(viewer.clone(), [trusted.clone(), followed_target.clone()]);
    graph.upsert_mute_list(trusted, [followed_target.clone()]);

    let trust = graph.compute_trust(&viewer, TrustConfig::default());
    let score = trust.score_for(&followed_target);

    assert_eq!(score.followed_by_viewer, true);
    assert_eq!(score.muted_by_trusted_count, 0);
    assert_eq!(score.decision, TrustDecision::Show);
    assert!(!trust.should_hide(&followed_target));
}

#[test]
fn direct_mute_hard_hides_even_when_author_is_nearby() {
    let viewer = pk(1);
    let muted = pk(2);
    let mut graph = SignalGraph::new();
    graph.upsert_follow_list(viewer.clone(), [muted.clone()]);
    graph.upsert_mute_list(viewer.clone(), [muted.clone()]);

    let trust = graph.compute_trust(&viewer, TrustConfig::default());
    let score = trust.score_for(&muted);

    assert_eq!(score.followed_by_viewer, true);
    assert_eq!(score.muted_by_viewer, true);
    assert_eq!(score.decision, TrustDecision::Hide);
}

#[test]
fn item_helpers_sort_and_filter_by_author_score() {
    let viewer = pk(1);
    let direct = pk(2);
    let hidden = pk(3);
    let unknown = pk(4);
    let mut graph = SignalGraph::new();
    graph.upsert_follow_list(viewer.clone(), [direct.clone()]);
    graph.upsert_mute_list(viewer.clone(), [hidden.clone()]);

    let trust = graph.compute_trust(&viewer, TrustConfig::default());
    let mut notes = vec![
        Note {
            author: unknown.clone(),
            body: "unknown",
        },
        Note {
            author: hidden.clone(),
            body: "hidden",
        },
        Note {
            author: direct.clone(),
            body: "direct",
        },
    ];

    trust.sort_by_author(&mut notes, |note| &note.author);
    assert_eq!(
        notes.iter().map(|note| note.body).collect::<Vec<_>>(),
        vec!["direct", "unknown", "hidden"]
    );

    let visible = trust.visible_items(notes, |note| &note.author);
    assert_eq!(
        visible
            .into_iter()
            .map(|note| note.body)
            .collect::<Vec<_>>(),
        vec!["direct", "unknown"]
    );
}
