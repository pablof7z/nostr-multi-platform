use super::*;
use nmp_threading::TimelineBlock;
use std::sync::Arc;

fn spec() -> ChirpHomeTimelineSpec {
    ChirpHomeTimelineSpec::for_viewer("me".into())
}

fn note(id: &str, ts: u64, tags: Vec<Vec<String>>) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: "auth".into(),
        kind: 1,
        created_at: ts,
        tags,
        content: id.into(),
    }
}

fn reply_to(id: &str, ts: u64, root: &str, parent: &str) -> KernelEvent {
    note(
        id,
        ts,
        vec![
            vec!["e".into(), root.into(), "".into(), "root".into()],
            vec!["e".into(), parent.into(), "".into(), "reply".into()],
        ],
    )
}

#[test]
fn view_module_open_yields_empty_snapshot() {
    let ctx = ViewContext::default();
    let (state, payload) = <ChirpHomeTimelineView as ViewModule>::open(&ctx, spec());
    assert!(payload.blocks.is_empty());
    assert!(payload.cards.is_empty());
    assert!(
        <ChirpHomeTimelineView as ViewModule>::snapshot(&ctx, &state)
            .blocks
            .is_empty()
    );
}

#[test]
fn view_module_ingests_events_and_snapshots_blocks_and_cards() {
    let ctx = ViewContext::default();
    let (mut state, _) = <ChirpHomeTimelineView as ViewModule>::open(&ctx, spec());
    <ChirpHomeTimelineView as ViewModule>::on_event_inserted(
        &ctx,
        &mut state,
        &note("R", 1, vec![]),
    );
    <ChirpHomeTimelineView as ViewModule>::on_event_inserted(
        &ctx,
        &mut state,
        &reply_to("C", 2, "R", "R"),
    );

    let snap = <ChirpHomeTimelineView as ViewModule>::snapshot(&ctx, &state);
    assert_eq!(snap.blocks.len(), 1);
    match &snap.blocks[0] {
        TimelineBlock::Module { events, .. } => {
            assert_eq!(events, &vec!["R".to_string(), "C".to_string()]);
        }
        other => panic!("expected Module, got {other:?}"),
    }
    assert_eq!(snap.cards.len(), 2);
}

#[test]
fn reset_by_opening_new_state_starts_empty() {
    let ctx = ViewContext::default();
    let (mut state, _) = <ChirpHomeTimelineView as ViewModule>::open(&ctx, spec());
    <ChirpHomeTimelineView as ViewModule>::on_event_inserted(
        &ctx,
        &mut state,
        &note("S", 1, vec![]),
    );
    assert!(
        !<ChirpHomeTimelineView as ViewModule>::snapshot(&ctx, &state)
            .blocks
            .is_empty()
    );

    let (fresh, fresh_payload) = <ChirpHomeTimelineView as ViewModule>::open(&ctx, spec());
    assert!(fresh_payload.blocks.is_empty());
    assert!(fresh_payload.cards.is_empty());
    assert!(
        <ChirpHomeTimelineView as ViewModule>::snapshot(&ctx, &fresh)
            .cards
            .is_empty()
    );
}

#[test]
fn rejected_events_do_not_change_blocks_or_cards() {
    let ctx = ViewContext::default();
    let (mut state, _) = <ChirpHomeTimelineView as ViewModule>::open(&ctx, spec());
    let mut profile = note("P", 1, vec![]);
    profile.kind = 0;

    let delta =
        <ChirpHomeTimelineView as ViewModule>::on_event_inserted(&ctx, &mut state, &profile);
    let snap = <ChirpHomeTimelineView as ViewModule>::snapshot(&ctx, &state);

    assert!(delta.is_none());
    assert!(snap.blocks.is_empty());
    assert!(snap.cards.is_empty());
}

#[test]
fn compatibility_observer_delegates_to_view_module() {
    let runtime: Arc<dyn KernelEventObserver> = Arc::new(ChirpHomeTimelineRuntime::new(spec()));
    runtime.on_kernel_event(&note("X", 1, vec![]));
}
