//! Aggregate + per-(reactor,target) newest-wins, exercised through the domain
//! store `reaction_summary` helper.

mod common;

use common::{reaction, repost};
use nmp_core::store::{EventStore, MemEventStore};
use nmp_reactions::{decode_and_route, reaction_summary, ReactionTarget, NAMESPACE};

const X: &str = "event-X-0000000000000000000000000000000000000000000000000000000000";
const Y: &str = "event-Y-0000000000000000000000000000000000000000000000000000000000";
const TA: &str = "target-author-00000000000000000000000000000000000000000000000000";

fn pk(name: &str) -> String {
    format!("{name}-{}", "0".repeat(60 - name.len()))
}

#[test]
fn three_users_thumbs_one_user_heart() {
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();

    for (i, name) in ["amy", "bob", "cat"].iter().enumerate() {
        let evt = reaction(
            &format!("{}{}", name, "0".repeat(64 - name.len())),
            &pk(name),
            100 + i as u64,
            X,
            TA,
            "👍",
        );
        decode_and_route(&evt, &handle).unwrap();
    }
    let heart = reaction(&"h".repeat(64), &pk("dan"), 200, X, TA, "❤️");
    decode_and_route(&heart, &handle).unwrap();

    let summary = reaction_summary(&handle, &ReactionTarget::Event(X.to_string())).unwrap();
    assert_eq!(summary.total, 4);
    let map: std::collections::HashMap<_, _> = summary.entries.iter().cloned().collect();
    assert_eq!(map.get("👍"), Some(&3));
    assert_eq!(map.get("❤️"), Some(&1));
    // Stable ordering: 👍 (3) before ❤️ (1).
    assert_eq!(summary.entries[0].0, "👍");
}

#[test]
fn per_reactor_newest_wins_switch_thumbs_to_heart() {
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();

    // Same reactor 👍 (ts=100) then ❤️ (ts=200) on X.
    let first = reaction(&"a".repeat(64), &pk("alice"), 100, X, TA, "👍");
    let second = reaction(&"b".repeat(64), &pk("alice"), 200, X, TA, "❤️");
    decode_and_route(&first, &handle).unwrap();
    decode_and_route(&second, &handle).unwrap();

    let summary = reaction_summary(&handle, &ReactionTarget::Event(X.to_string())).unwrap();
    assert_eq!(summary.total, 1, "one distinct reactor");
    assert_eq!(
        summary.entries,
        vec![("❤️".to_string(), 1)],
        "newest reaction (❤️) wins; the older 👍 is not also counted"
    );
}

#[test]
fn summary_cross_target_isolation() {
    // A reaction on Y must not appear in the summary for X.
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();

    let on_x = reaction(&"a".repeat(64), &pk("alice"), 100, X, TA, "👍");
    let on_y = reaction(&"b".repeat(64), &pk("bob"), 100, Y, TA, "👍");
    decode_and_route(&on_x, &handle).unwrap();
    decode_and_route(&on_y, &handle).unwrap();

    let sx = reaction_summary(&handle, &ReactionTarget::Event(X.to_string())).unwrap();
    let sy = reaction_summary(&handle, &ReactionTarget::Event(Y.to_string())).unwrap();
    assert_eq!(sx.total, 1, "X has exactly one reaction");
    assert_eq!(sy.total, 1, "Y has its own reaction, isolated from X");
}

#[test]
fn summary_excludes_reposts_from_the_reaction_aggregate() {
    // A repost (kind:6) and a ❤️ reaction (kind:7) on X by different authors.
    // Reposts are a SEPARATE surface (RepostsView / list_for_target filtered to
    // is_repost), never folded into the reaction aggregate. The domain-side
    // summary must match the view-side accumulator exactly: total=1, ❤️ only.
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();

    let rp = repost(&"a".repeat(64), &pk("alice"), 100, X, TA, "");
    let heart = reaction(&"b".repeat(64), &pk("bob"), 100, X, TA, "❤️");
    decode_and_route(&rp, &handle).unwrap();
    decode_and_route(&heart, &handle).unwrap();

    let s = reaction_summary(&handle, &ReactionTarget::Event(X.to_string())).unwrap();
    assert_eq!(
        s.total, 1,
        "only the reaction counts; the repost is excluded"
    );
    assert_eq!(s.entries, vec![("❤️".to_string(), 1)]);
}
