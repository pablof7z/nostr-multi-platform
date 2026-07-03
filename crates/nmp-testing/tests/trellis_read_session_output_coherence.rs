use trellis_core::{
    ClearReason, DependencyList, Graph, MaterializedOutput, OutputFrame, OutputFrameKind,
    OutputKey, Revision,
};

#[derive(Clone, Debug, Eq, PartialEq)]
struct ReadSessionSource {
    projection_key: String,
    viewer_pubkey: String,
    event_ids: Vec<String>,
    relay_hints: Vec<String>,
    covered_until: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ReadSessionOutput {
    projection_key: String,
    viewer_pubkey: String,
    rows: Vec<ReadSessionRow>,
    relay_hints: Vec<String>,
    covered_until: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ReadSessionRow {
    position: u32,
    event_id: String,
}

#[derive(Default)]
struct HostReadSession {
    key: Option<OutputKey>,
    revision: Option<Revision>,
    state: Option<ReadSessionOutput>,
}

impl HostReadSession {
    fn apply(&mut self, frame: &OutputFrame) {
        assert_eq!(
            self.key
                .replace(frame.output_key)
                .unwrap_or(frame.output_key),
            frame.output_key,
            "one host read session must not receive frames for another output"
        );
        if let Some(previous) = self.revision {
            assert!(
                frame.revision > previous,
                "read-session output revisions must be strictly monotonic"
            );
        }
        self.revision = Some(frame.revision);

        match &frame.kind {
            OutputFrameKind::Baseline(_)
            | OutputFrameKind::Delta(_)
            | OutputFrameKind::Rebaseline(_, _) => {
                let value = frame
                    .kind
                    .payload::<ReadSessionOutput>()
                    .expect("read-session output payload must be typed");
                self.state = Some(value.clone());
            }
            OutputFrameKind::Clear(_) => {
                self.state = None;
            }
        }
    }
}

fn source(event_ids: &[&str], covered_until: u64) -> ReadSessionSource {
    ReadSessionSource {
        projection_key: "read.session.primary".to_owned(),
        viewer_pubkey: "viewer-pubkey".to_owned(),
        event_ids: event_ids.iter().map(|id| (*id).to_owned()).collect(),
        relay_hints: vec![
            "wss://relay-one.example".to_owned(),
            "wss://relay-two.example".to_owned(),
        ],
        covered_until,
    }
}

fn materialize(source: &ReadSessionSource) -> ReadSessionOutput {
    ReadSessionOutput {
        projection_key: source.projection_key.clone(),
        viewer_pubkey: source.viewer_pubkey.clone(),
        rows: source
            .event_ids
            .iter()
            .enumerate()
            .map(|(index, event_id)| ReadSessionRow {
                position: index as u32,
                event_id: event_id.clone(),
            })
            .collect(),
        relay_hints: source.relay_hints.clone(),
        covered_until: source.covered_until,
    }
}

fn single_frame(frames: &[OutputFrame]) -> &OutputFrame {
    assert_eq!(
        frames.len(),
        1,
        "transaction must emit exactly one output frame"
    );
    &frames[0]
}

#[test]
fn trellis_read_session_output_clear_rebaseline_delta_coherence() {
    let mut graph = Graph::<()>::new();

    let mut tx = graph.begin_transaction().expect("begin open transaction");
    let scope = tx.create_scope("read-session").expect("create scope");
    let read_source = tx
        .input::<ReadSessionSource>("read-source")
        .expect("create read source");
    let initial_source = source(&["event-a", "event-b"], 10);
    tx.set_input(read_source, initial_source.clone())
        .expect("seed source");
    let output: MaterializedOutput<ReadSessionOutput> = tx
        .materialized_output(
            "read-session-output",
            scope,
            DependencyList::new([read_source.id()]).expect("declared dependency"),
            move |ctx| Ok(materialize(ctx.input(read_source)?)),
        )
        .expect("create materialized read-session output");
    let output_key = output.key();
    let opened = tx.commit().expect("commit open transaction");
    drop(tx);

    let expected_initial = materialize(&initial_source);
    let frame = single_frame(&opened.output_frames);
    assert_eq!(frame.output_key, output_key);
    assert_eq!(frame.scope, scope);
    assert!(matches!(
        &frame.kind,
        OutputFrameKind::Baseline(_) if frame.kind.payload::<ReadSessionOutput>() == Some(&expected_initial)
    ));

    let mut host = HostReadSession::default();
    host.apply(frame);
    assert_eq!(host.state.as_ref(), Some(&expected_initial));

    let delta_source = source(&["event-a", "event-c", "event-d"], 20);
    let mut tx = graph.begin_transaction().expect("begin delta transaction");
    tx.set_input(read_source, delta_source.clone())
        .expect("update source");
    let changed = tx.commit().expect("commit delta transaction");
    drop(tx);

    let expected_delta = materialize(&delta_source);
    let frame = single_frame(&changed.output_frames);
    assert!(matches!(
        &frame.kind,
        OutputFrameKind::Delta(_) if frame.kind.payload::<ReadSessionOutput>() == Some(&expected_delta)
    ));
    host.apply(frame);
    assert_eq!(
        host.state.as_ref(),
        Some(&expected_delta),
        "applying the Trellis delta must reconstruct the same typed read-session state"
    );

    let mut tx = graph
        .begin_transaction()
        .expect("begin rebaseline transaction");
    tx.rebaseline_output(output.clone())
        .expect("request rebaseline");
    let rebaselined = tx.commit().expect("commit rebaseline transaction");
    drop(tx);

    let frame = single_frame(&rebaselined.output_frames);
    assert!(matches!(
        &frame.kind,
        OutputFrameKind::Rebaseline(_, trellis_core::RebaselineReason::Requested)
            if frame.kind.payload::<ReadSessionOutput>() == Some(&expected_delta)
    ));
    host.apply(frame);
    assert_eq!(
        host.state.as_ref(),
        Some(&expected_delta),
        "a rebaseline must agree with the host state reconstructed from prior deltas"
    );

    let post_rebaseline_source = source(&["event-c"], 30);
    let mut tx = graph
        .begin_transaction()
        .expect("begin post-rebaseline delta transaction");
    tx.set_input(read_source, post_rebaseline_source.clone())
        .expect("update source after rebaseline");
    let post_rebaseline = tx.commit().expect("commit post-rebaseline delta");
    drop(tx);

    let expected_post_rebaseline = materialize(&post_rebaseline_source);
    let frame = single_frame(&post_rebaseline.output_frames);
    assert!(matches!(
        &frame.kind,
        OutputFrameKind::Delta(_)
            if frame.kind.payload::<ReadSessionOutput>() == Some(&expected_post_rebaseline)
    ));
    host.apply(frame);
    assert_eq!(host.state.as_ref(), Some(&expected_post_rebaseline));

    let mut tx = graph.begin_transaction().expect("begin close transaction");
    tx.close_scope(scope).expect("close read-session scope");
    let closed = tx.commit().expect("commit close transaction");
    drop(tx);

    let frame = single_frame(&closed.output_frames);
    assert_eq!(frame.output_key, output_key);
    assert!(matches!(
        frame.kind,
        OutputFrameKind::Clear(ClearReason::ScopeClosed)
    ));
    host.apply(frame);
    assert_eq!(
        host.state, None,
        "closing the Trellis scope must clear the host read-session projection"
    );
    assert!(
        graph.output_meta(output_key).is_none(),
        "closed scope must remove the materialized output registration"
    );
}
