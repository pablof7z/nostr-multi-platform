use std::collections::BTreeSet;

use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;
use nmp_kinds::KIND_LONG_FORM_ARTICLE;

use super::*;
use crate::{AddressCoordinate, KIND_DELETE, KIND_GENERIC_REPOST, KIND_REPOST};

fn event(
    id: &str,
    author: &str,
    kind: u32,
    created_at: u64,
    tags: Vec<Vec<&str>>,
    content: &str,
) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind,
        created_at,
        tags: tags
            .into_iter()
            .map(|tag| tag.into_iter().map(str::to_string).collect())
            .collect(),
        content: content.to_string(),
        relay_provenance: Vec::new(),
    }
}

fn authors(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn aggregates_longform_article_reposts_by_author_set() {
    let projection = RepostActivityProjection::new();
    projection.ingest_event(&event(
        "r1",
        "alice",
        KIND_GENERIC_REPOST,
        10,
        vec![
            vec!["a", "30023:bob:article-a"],
            vec!["k", &KIND_LONG_FORM_ARTICLE.to_string()],
        ],
        "",
    ));
    projection.ingest_event(&event(
        "r2",
        "carol",
        KIND_GENERIC_REPOST,
        20,
        vec![
            vec!["a", "30023:bob:article-a"],
            vec!["k", &KIND_LONG_FORM_ARTICLE.to_string()],
        ],
        "",
    ));
    projection.ingest_event(&event(
        "r3",
        "mallory",
        KIND_GENERIC_REPOST,
        30,
        vec![
            vec!["a", "30023:bob:article-b"],
            vec!["k", &KIND_LONG_FORM_ARTICLE.to_string()],
        ],
        "",
    ));

    let activity = projection.article_activity_for_authors(&authors(&["alice", "carol"]));

    assert_eq!(activity.len(), 1);
    assert_eq!(
        activity[0].target,
        RepostTarget::Address(AddressCoordinate::new(
            KIND_LONG_FORM_ARTICLE,
            "bob",
            "article-a"
        ))
    );
    assert_eq!(activity[0].interactor_pubkeys, authors(&["alice", "carol"]));
    assert_eq!(activity[0].latest_activity_at, 20);
}

#[test]
fn target_kind_filter_excludes_unrelated_reposts() {
    let projection = RepostActivityProjection::new();
    projection.ingest_event(&event(
        "short-repost",
        "alice",
        KIND_REPOST,
        10,
        vec![vec!["e", "note"], vec!["k", "1"]],
        "",
    ));
    projection.ingest_event(&event(
        "article-repost",
        "alice",
        KIND_GENERIC_REPOST,
        20,
        vec![
            vec!["a", "30023:bob:article-a"],
            vec!["k", &KIND_LONG_FORM_ARTICLE.to_string()],
        ],
        "",
    ));

    let article_targets = projection.article_targets_reposted_by_authors(&authors(&["alice"]));

    assert_eq!(
        article_targets,
        BTreeSet::from([RepostTarget::Address(AddressCoordinate::new(
            KIND_LONG_FORM_ARTICLE,
            "bob",
            "article-a"
        ))])
    );
}

#[test]
fn event_id_only_repost_is_queryable_when_kind_is_proven() {
    let projection = RepostActivityProjection::new();
    projection.on_kernel_event(&event(
        "r1",
        "alice",
        KIND_REPOST,
        10,
        vec![vec!["e", "target-note"], vec!["k", "1"]],
        "",
    ));

    let activity = projection.activity_for_authors(&authors(&["alice"]), Some(1));

    assert_eq!(activity.len(), 1);
    assert_eq!(
        activity[0].target,
        RepostTarget::Event {
            event_id: "target-note".to_string(),
            kind: Some(1),
        }
    );
}

#[test]
fn delete_by_same_author_retracts_repost_wrapper() {
    let projection = RepostActivityProjection::new();
    projection.ingest_event(&event(
        "r1",
        "alice",
        KIND_GENERIC_REPOST,
        10,
        vec![
            vec!["a", "30023:bob:article-a"],
            vec!["k", &KIND_LONG_FORM_ARTICLE.to_string()],
        ],
        "",
    ));
    projection.ingest_event(&event(
        "d1",
        "alice",
        KIND_DELETE,
        11,
        vec![vec!["e", "r1"]],
        "",
    ));

    assert!(projection
        .article_activity_for_authors(&authors(&["alice"]))
        .is_empty());
    assert!(projection.is_empty());
}

#[test]
fn foreign_delete_does_not_retract_repost_wrapper() {
    let projection = RepostActivityProjection::new();
    projection.ingest_event(&event(
        "r1",
        "alice",
        KIND_REPOST,
        10,
        vec![vec!["e", "note"], vec!["k", "1"]],
        "",
    ));
    projection.ingest_event(&event(
        "d1",
        "mallory",
        KIND_DELETE,
        11,
        vec![vec!["e", "r1"]],
        "",
    ));

    assert_eq!(
        projection
            .activity_for_authors(&authors(&["alice"]), Some(1))
            .len(),
        1
    );
}

#[test]
fn projection_is_bounded_by_repost_event_id() {
    let projection = RepostActivityProjection::with_capacity(1);
    projection.ingest_event(&event(
        "r1",
        "alice",
        KIND_REPOST,
        10,
        vec![vec!["e", "old"], vec!["k", "1"]],
        "",
    ));
    projection.ingest_event(&event(
        "r2",
        "alice",
        KIND_REPOST,
        20,
        vec![vec!["e", "new"], vec!["k", "1"]],
        "",
    ));

    let targets = projection.targets_reposted_by_authors(&authors(&["alice"]), Some(1));

    assert_eq!(
        targets,
        BTreeSet::from([RepostTarget::Event {
            event_id: "new".to_string(),
            kind: Some(1),
        }])
    );
    assert_eq!(projection.len(), 1);
}

#[test]
fn interest_shape_covers_repost_wrappers_and_deletes() {
    let shape = repost_activity_interest_shape(authors(&["alice", "bob"])).unwrap();

    assert_eq!(shape.authors, authors(&["alice", "bob"]));
    assert_eq!(
        shape.kinds,
        BTreeSet::from([KIND_REPOST, KIND_GENERIC_REPOST, KIND_DELETE])
    );
    assert!(repost_activity_interest_shape(BTreeSet::new()).is_none());
}
