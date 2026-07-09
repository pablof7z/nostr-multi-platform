//! #3087 `DeliveredRefDemand` retraction proofs, split out of
//! `composite_compiler_tests.rs` to keep it under the file-size gate
//! (AGENTS.md). A child module of `tests` — reuses its fixtures
//! (`article`/`comment`/`repost`/`DrivingExample`/`CompiledLane`/
//! `author_admission`) via `use super::*;`.

use super::*;

/// #3087 end-to-end proof: the composite compiler wires `DeliveredRefDemand`
/// to `FlatFeed`'s `SourceRemovedHook` exactly the way `open_composite_feed`
/// does (`source_removed` in that function). This drives the REAL mechanism —
/// not just the `delivered_ref` unit — to prove a declaring event's own
/// source contribution being removed (delete/mute) actually retracts its
/// target's demand through the wired engine, and that the OLD monotonic
/// behavior (demand never shrinks once a declaring event is gone) is fixed.
///
/// Note the comment's row id is the ARTICLE's own coordinate, not a separate
/// identity: `nip22_root_mapping` keys its `MappedRow` by
/// `target.canonical_key()`, so the comment row-merges onto the SAME
/// canonical row the article itself will later occupy (#3082's
/// `ByTargetCreatedAt` policy). The comment is still an independently
/// removable SOURCE within that row (`FlatFeedItem::source_id` = the
/// comment's own event id) — which is exactly why demand must be keyed by
/// the declaring EVENT's id, not the row id.
#[test]
fn removing_the_declaring_source_retracts_its_delivered_ref_demand_through_the_wired_engine() {
    let author = "article-author".to_string();
    let commenter = "commenter".to_string();
    let follows: BTreeSet<String> = [author.clone(), commenter.clone()].into_iter().collect();
    let d_tag = "muted-article";
    let coordinate = format!("{KIND_ARTICLE}:{author}:{d_tag}");
    let target = nmp_feed::TypedRefTarget::Address {
        kind: KIND_ARTICLE,
        pubkey: author.clone(),
        d: d_tag.to_string(),
    };

    let example = driving_example_with_source_removed_hook(follows);

    // The comment is the DECLARING source: its lane mapping's `Delivered` ref
    // to the article registers demand keyed by the comment's OWN event id
    // ("comment-1"), even though its row (per `nip22_root_mapping`) is keyed
    // by the article's coordinate.
    let comment_event = comment(&commenter, &author, d_tag, 200, "comment-1");
    example.feed.on_kernel_event(&comment_event);

    assert!(
        example.demand.targets().contains(&target),
        "the comment source's Delivered ref demands the article"
    );
    let admit_before = union_admission(&example.demand, vec![KIND_ARTICLE]);
    assert!(
        admit_before(&article(&author, d_tag, 42, "article-1", "body")),
        "the article is admitted while the comment source still declares demand for it"
    );

    // The comment is muted/deleted — its source contribution is removed from
    // the shared row via the SAME `FlatFeed::remove_source` primitive a real
    // mute/delete observer would call (row id = the article's coordinate,
    // source id = the comment's own event id).
    assert!(
        example.feed.remove_source(&coordinate, "comment-1"),
        "the comment's source contribution must exist to be removed"
    );

    // #3087: retraction wired through the engine's SourceRemovedHook must
    // have fired synchronously inside `remove_source` — no separate step
    // needed.
    assert!(
        !example.demand.targets().contains(&target),
        "removing the ONLY declaring source must retract its target's demand \
         (pre-#3087 this leaked forever: demand only ever incremented)"
    );
    let admit_after = union_admission(&example.demand, vec![KIND_ARTICLE]);
    assert!(
        !admit_after(&article(&author, d_tag, 42, "article-2", "body")),
        "a retracted target's subscription must no longer admit its delivery"
    );
    assert!(
        union_live_shape(&example.demand, vec![KIND_ARTICLE])().is_none(),
        "a retracted target must not appear in the live acquisition shape either"
    );
}

/// Two declaring sources (a comment AND a repost, both pointing at the same
/// article) share the demand; removing one must not retract the other's live
/// demand, and removing the last one must retract it — proving the refcount
/// is per-DECLARER, not a bare monotonic flag, through the wired engine.
#[test]
fn removing_one_of_two_declaring_sources_leaves_the_others_demand_intact() {
    let author = "article-author".to_string();
    let commenter = "commenter".to_string();
    let reposter = "reposter".to_string();
    let follows: BTreeSet<String> = [author.clone(), commenter.clone(), reposter.clone()]
        .into_iter()
        .collect();
    let d_tag = "shared-article";
    let coordinate = format!("{KIND_ARTICLE}:{author}:{d_tag}");
    let target = nmp_feed::TypedRefTarget::Address {
        kind: KIND_ARTICLE,
        pubkey: author.clone(),
        d: d_tag.to_string(),
    };

    let example = driving_example_with_source_removed_hook(follows);
    example
        .feed
        .on_kernel_event(&comment(&commenter, &author, d_tag, 200, "comment-1"));
    example
        .feed
        .on_kernel_event(&repost(&reposter, &author, d_tag, 210, "repost-1"));
    assert!(example.demand.targets().contains(&target));

    assert!(example.feed.remove_source(&coordinate, "comment-1"));
    assert!(
        example.demand.targets().contains(&target),
        "the repost's own demand for the SAME target must survive the comment's removal"
    );

    assert!(example.feed.remove_source(&coordinate, "repost-1"));
    assert!(
        !example.demand.targets().contains(&target),
        "removing the LAST declaring source must retract the target"
    );
}

/// Same wiring as [`driving_example`], but scoped to just the comment lane and
/// plus the `SourceRemovedHook` the real `open_composite_feed` registers so
/// `FlatFeed::remove_item`/`remove_source`/`remove_sources_if` retract
/// `DeliveredRefDemand` in lockstep (#3087). `driving_example` itself stays
/// hook-free — most of its assertions are about row-building/merge, not
/// retraction, and adding the hook there would be an unrelated behavior
/// change to every test that uses it.
fn driving_example_with_source_removed_hook(follows: BTreeSet<String>) -> DrivingExample {
    let lanes = vec![
        CompiledLane {
            admission: author_admission(follows.clone()),
            match_kinds: BTreeSet::from([KIND_COMMENT]),
            match_tags: [("K".to_string(), BTreeSet::from([KIND_ARTICLE.to_string()]))]
                .into_iter()
                .collect(),
            mapping: nmp_nip22::nip22_root_mapping(),
        },
        CompiledLane {
            admission: author_admission(follows),
            match_kinds: BTreeSet::from([KIND_REPOST]),
            match_tags: [("k".to_string(), BTreeSet::from([KIND_ARTICLE.to_string()]))]
                .into_iter()
                .collect(),
            mapping: nmp_nip18::nip18_target_mapping(),
        },
    ];
    let lanes = Arc::new(lanes);
    let demand = DeliveredRefDemand::new();
    let render_target_kinds = vec![KIND_ARTICLE];

    let admission = {
        let lanes = Arc::clone(&lanes);
        let demand_admits = union_admission(&demand, render_target_kinds.clone());
        Arc::new(move |event: &KernelEvent| {
            lanes.iter().any(|lane| lane_claims(lane, event)) || demand_admits(event)
        })
    };
    let item_builder = {
        let lanes = Arc::clone(&lanes);
        let demand = Arc::clone(&demand);
        let render_target_kinds = render_target_kinds.clone();
        Arc::new(move |event: &KernelEvent| {
            build_composite_rows(&lanes, &demand, &render_target_kinds, event)
        })
    };
    let merge = composite_merge(SortPolicy::ByTargetCreatedAt);
    let source_removed: nmp_feed::SourceRemovedHook = {
        let demand = Arc::clone(&demand);
        Arc::new(move |source_id: &str| {
            demand.retract_source(source_id);
        })
    };

    let feed = FlatFeed::with_merge_window_policy_and_source_removed_hook(
        admission,
        item_builder,
        None,
        merge,
        nmp_feed::FeedWindowPolicy::default(),
        Some(source_removed),
    );
    DrivingExample { feed, demand }
}
