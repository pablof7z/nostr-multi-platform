use std::collections::BTreeSet;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use nmp_feed::FeedShape;

use crate::trellis_adapter::FeedSessionTrellisAdapter;
use crate::trellis_adapter_equivalence_support::{
    assert_changed_step, assert_unchanged_step, command_receiver, delta_close_authors,
    drain_delta_commands, drain_mark_changed, expected_traces, extra_from_authors,
    remove_projection_action, trace_delta, OldReplacementPath,
};
use crate::trellis_adapter_trace::{FeedSessionOutputFrameKind, FeedSessionResourceTraceKind};

#[test]
fn adapter_matches_old_path_and_full_recompute_across_source_prefixes() {
    let (sender, rx) = command_receiver();
    let adapter =
        FeedSessionTrellisAdapter::new("app.feed.equivalence", FeedShape::Flat, Vec::new(), sender)
            .unwrap();
    let mut old = OldReplacementPath::default();

    assert_unchanged_step(&adapter, &rx, &mut old, &[], "initial-empty");
    assert_changed_step(
        &adapter,
        &rx,
        &mut old,
        &["alice", "bob"],
        "initial-open",
        expected_traces(FeedSessionResourceTraceKind::Open, &["alice", "bob"]),
    );
    assert_changed_step(
        &adapter,
        &rx,
        &mut old,
        &["alice", "bob", "carol"],
        "source-expansion",
        expected_traces(FeedSessionResourceTraceKind::Open, &["carol"]),
    );
    assert_changed_step(
        &adapter,
        &rx,
        &mut old,
        &["alice"],
        "source-shrink",
        expected_traces(FeedSessionResourceTraceKind::Close, &["bob", "carol"]),
    );
    assert_unchanged_step(&adapter, &rx, &mut old, &["alice"], "source-shrink-noop");
    assert_changed_step(
        &adapter,
        &rx,
        &mut old,
        &[],
        "empty-source",
        expected_traces(FeedSessionResourceTraceKind::Close, &["alice"]),
    );
    assert_unchanged_step(&adapter, &rx, &mut old, &[], "empty-source-noop");
    assert_changed_step(
        &adapter,
        &rx,
        &mut old,
        &["dave", "erin"],
        "active-account-switch",
        expected_traces(FeedSessionResourceTraceKind::Open, &["dave", "erin"]),
    );

    assert_changed_step(
        &adapter,
        &rx,
        &mut old,
        &["alice", "dave"],
        "replaceable-contact-list-update",
        expected_traces(FeedSessionResourceTraceKind::Open, &["alice"])
            .into_iter()
            .chain(expected_traces(
                FeedSessionResourceTraceKind::Close,
                &["erin"],
            ))
            .collect(),
    );

    let trace_start = adapter.resource_traces_for_test().len();
    assert!(!adapter.rebaseline_output_if_changed(false));
    assert_eq!(drain_mark_changed(&rx), 0);
    assert!(trace_delta(&adapter, trace_start).is_empty());

    assert!(adapter.rebaseline_output_if_changed(true));
    assert_eq!(drain_mark_changed(&rx), 1);
    assert!(trace_delta(&adapter, trace_start).is_empty());
    assert_eq!(
        adapter.output_frame_kinds_for_test(),
        vec![
            FeedSessionOutputFrameKind::Baseline,
            FeedSessionOutputFrameKind::Rebaseline,
        ],
        "unchanged output must not churn; changed output rebaselines exactly once"
    );

    let remove_count = Arc::new(AtomicUsize::new(0));
    let trace_start = adapter.resource_traces_for_test().len();
    (adapter.close_action(remove_projection_action(Arc::clone(&remove_count))))();
    assert_eq!(remove_count.load(Ordering::SeqCst), 1);
    let deltas = drain_delta_commands(&rx);
    assert_eq!(deltas.len(), 1);
    assert_eq!(deltas[0].1, "feed-session-acquisition-close");
    assert_eq!(
        delta_close_authors(&deltas[0].0),
        BTreeSet::from(["alice".to_string(), "dave".to_string()])
    );
    assert_eq!(
        trace_delta(&adapter, trace_start),
        expected_traces(FeedSessionResourceTraceKind::Close, &["alice", "dave"])
    );
    assert_eq!(
        adapter.output_frame_kinds_for_test(),
        vec![
            FeedSessionOutputFrameKind::Baseline,
            FeedSessionOutputFrameKind::Rebaseline,
            FeedSessionOutputFrameKind::Clear,
        ],
        "close must clear the materialized output once"
    );

    let trace_start = adapter.resource_traces_for_test().len();
    assert!(!adapter.sync(&extra_from_authors(&["frank"]), "late-source-after-close"));
    assert!(drain_delta_commands(&rx).is_empty());
    assert!(!adapter.rebaseline_output_if_changed(true));
    assert_eq!(drain_mark_changed(&rx), 0);
    assert!(trace_delta(&adapter, trace_start).is_empty());

    (adapter.close_action(remove_projection_action(Arc::clone(&remove_count))))();
    assert_eq!(remove_count.load(Ordering::SeqCst), 1);
    assert!(drain_delta_commands(&rx).is_empty());
}

#[test]
fn local_source_change_does_not_replan_unrelated_session_adapter() {
    let (sender, rx) = command_receiver();
    let active_adapter = FeedSessionTrellisAdapter::new(
        "app.feed.active",
        FeedShape::Flat,
        Vec::new(),
        sender.clone(),
    )
    .unwrap();
    let static_adapter =
        FeedSessionTrellisAdapter::new("app.feed.static", FeedShape::Flat, Vec::new(), sender)
            .unwrap();
    let mut active_old = OldReplacementPath::default();
    let mut static_old = OldReplacementPath::default();

    assert_changed_step(
        &active_adapter,
        &rx,
        &mut active_old,
        &["alice"],
        "active-initial",
        expected_traces(FeedSessionResourceTraceKind::Open, &["alice"]),
    );
    assert_changed_step(
        &static_adapter,
        &rx,
        &mut static_old,
        &["static-author"],
        "static-initial",
        expected_traces(FeedSessionResourceTraceKind::Open, &["static-author"]),
    );

    let static_trace_start = static_adapter.resource_traces_for_test().len();
    let static_output_frames = static_adapter.output_frame_kinds_for_test();
    assert_changed_step(
        &active_adapter,
        &rx,
        &mut active_old,
        &["alice", "bob"],
        "active-local-expansion",
        expected_traces(FeedSessionResourceTraceKind::Open, &["bob"]),
    );

    assert!(
        trace_delta(&static_adapter, static_trace_start).is_empty(),
        "local source changes must not emit unrelated session resource plans"
    );
    assert_eq!(
        static_adapter.output_frame_kinds_for_test(),
        static_output_frames,
        "local source changes must not churn unrelated session output frames"
    );
}
