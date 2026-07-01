use std::collections::BTreeSet;

use trellis_core::{
    CollectionNode, DependencyList, Graph, HostResourceOutcome, InputNode, MaterializedOutput,
    OutputFrameKind, OutputKey, ResourceCommandKind, ResourceCommandTrace, ResourceKey,
    ResourcePlan, ResourceTransitionPolicy, ScopeId,
};
use trellis_testing::{
    assert_dependency_path_exists, assert_every_output_frame_has_revision,
    assert_every_output_frame_has_scope, assert_every_resource_command_has_cause,
    assert_incremental_equals_full, assert_no_unexplained_output_frame, assert_no_unexplained_plan,
    ConformanceLevel, ConformanceSuite, FakeHost, FullRecomputeOracle, HostStatusClass,
    HostStatusEvent, NoRedaction, OutputLedger, ResourceLedger, Scenario,
};

type Rows = BTreeSet<TimelineRow>;

#[derive(Clone, Debug, Eq, PartialEq)]
enum ReadCommand {
    OpenRelayInterest {
        relay_url: String,
        filter_json: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct InterestKey {
    author: &'static str,
    relay_url: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct TimelineRow {
    author: &'static str,
    relay_url: &'static str,
}

struct ReadSessionGraph {
    graph: Graph<ReadCommand, Rows>,
    sources: InputNode<BTreeSet<InterestKey>>,
    host_statuses: InputNode<Vec<HostStatusEvent>>,
    demand: CollectionNode<InterestKey, ()>,
    output: MaterializedOutput<Rows>,
    scope: ScopeId,
}

fn interest(author: &'static str) -> InterestKey {
    InterestKey {
        author,
        relay_url: "wss://relay.example",
    }
}

fn sources(authors: &[&'static str]) -> BTreeSet<InterestKey> {
    authors.iter().copied().map(interest).collect()
}

fn rows_from_sources(sources: &BTreeSet<InterestKey>) -> Rows {
    sources
        .iter()
        .map(|source| TimelineRow {
            author: source.author,
            relay_url: source.relay_url,
        })
        .collect()
}

fn resource_key(source: &InterestKey) -> ResourceKey {
    ResourceKey::new(format!(
        "relay-interest:{}#author={}",
        source.relay_url, source.author
    ))
}

fn read_command(source: &InterestKey) -> ReadCommand {
    ReadCommand::OpenRelayInterest {
        relay_url: source.relay_url.to_owned(),
        filter_json: format!(r#"{{"authors":["{}"],"kinds":[1]}}"#, source.author),
    }
}

fn build_graph(
    initial_sources: BTreeSet<InterestKey>,
) -> (
    ReadSessionGraph,
    trellis_core::TransactionResult<ReadCommand, Rows>,
) {
    let mut graph = Graph::<ReadCommand, Rows>::new_with_command_type();
    let mut tx = graph.begin_transaction().unwrap();
    let scope = tx.create_scope("nmp.timeline.read-session").unwrap();
    let sources = tx
        .input::<BTreeSet<InterestKey>>("app-owned-source-set")
        .unwrap();
    let host_statuses = tx
        .input::<Vec<HostStatusEvent>>("host-resource-status-feedback")
        .unwrap();

    tx.set_input(sources, initial_sources).unwrap();
    tx.set_input(host_statuses, Vec::new()).unwrap();
    tx.attach_node_to_scope(sources, scope).unwrap();
    tx.attach_node_to_scope(host_statuses, scope).unwrap();

    let demand = tx
        .set_collection(
            "rust-owned-relay-interest-demand",
            DependencyList::new([sources.id()]).unwrap(),
            move |ctx| Ok(ctx.input(sources)?.clone()),
        )
        .unwrap();
    tx.attach_node_to_scope(demand, scope).unwrap();
    tx.set_resource_planner(demand, scope, move |ctx| {
        let mut plan = ResourcePlan::new();
        for added in &ctx.diff().added {
            plan.open(
                resource_key(&added.value),
                ctx.scope(),
                read_command(&added.value),
            );
        }
        for removed in &ctx.diff().removed {
            plan.close(resource_key(&removed.value), ctx.scope());
        }
        Ok(plan)
    })
    .unwrap();

    let output = tx
        .materialized_output(
            "typed-timeline-rows",
            scope,
            DependencyList::new([demand.id()]).unwrap(),
            move |ctx| Ok(rows_from_sources(ctx.set_collection(demand)?)),
        )
        .unwrap();
    let result = tx.commit().unwrap();
    drop(tx);

    (
        ReadSessionGraph {
            graph,
            sources,
            host_statuses,
            demand,
            output,
            scope,
        },
        result,
    )
}

fn set_sources(
    target: &mut ReadSessionGraph,
    values: BTreeSet<InterestKey>,
) -> trellis_core::TransactionResult<ReadCommand, Rows> {
    let mut tx = target.graph.begin_transaction().unwrap();
    tx.set_input(target.sources, values).unwrap();
    let result = tx.commit().unwrap();
    drop(tx);
    target.graph.assert_incremental_equals_full().unwrap();
    result
}

fn set_host_statuses(
    target: &mut ReadSessionGraph,
    statuses: Vec<HostStatusEvent>,
) -> trellis_core::TransactionResult<ReadCommand, Rows> {
    let mut tx = target.graph.begin_transaction().unwrap();
    tx.set_input(target.host_statuses, statuses).unwrap();
    let result = tx.commit().unwrap();
    drop(tx);
    target.graph.assert_incremental_equals_full().unwrap();
    result
}

fn rebaseline_rows(
    target: &mut ReadSessionGraph,
) -> trellis_core::TransactionResult<ReadCommand, Rows> {
    let mut tx = target.graph.begin_transaction().unwrap();
    tx.rebaseline_output(target.output.clone()).unwrap();
    let result = tx.commit().unwrap();
    drop(tx);
    target.graph.assert_incremental_equals_full().unwrap();
    result
}

fn close_scope(
    target: &mut ReadSessionGraph,
) -> trellis_core::TransactionResult<ReadCommand, Rows> {
    let mut tx = target.graph.begin_transaction().unwrap();
    tx.close_scope(target.scope).unwrap();
    let result = tx.commit().unwrap();
    drop(tx);
    target.graph.assert_incremental_equals_full().unwrap();
    result
}

fn read_session_scenario() -> Scenario {
    let (mut target, initial) = build_graph(sources(&["alice", "bob"]));
    let mut scenario = Scenario::new();
    scenario.record("initial", &initial);
    let shrink = set_sources(&mut target, sources(&["alice"]));
    scenario.record("source-shrink", &shrink);
    let empty = set_sources(&mut target, BTreeSet::new());
    scenario.record("empty-source", &empty);
    scenario
}

struct OutputOracle;

impl FullRecomputeOracle<OutputLedger<Rows>> for OutputOracle {
    type CanonicalInputs = (OutputKey, Rows);
    type ExpectedState = Rows;

    fn recompute(inputs: &Self::CanonicalInputs) -> Self::ExpectedState {
        inputs.1.clone()
    }

    fn observe_incremental(
        ledger: &OutputLedger<Rows>,
        inputs: &Self::CanonicalInputs,
    ) -> Self::ExpectedState {
        ledger
            .snapshot(inputs.0)
            .and_then(|snapshot| snapshot.state.clone())
            .unwrap_or_default()
    }
}

#[test]
fn source_shrink_withdraws_scoped_demand_without_broad_fallback() {
    let (mut target, initial) = build_graph(sources(&["alice", "bob"]));
    let alice = interest("alice");
    let bob = interest("bob");
    let alice_key = resource_key(&alice);
    let bob_key = resource_key(&bob);

    let mut ledger = ResourceLedger::new();
    ledger.mark_forbidden_unless_explicit(ResourceKey::wildcard("timeline"));
    ledger.apply_result(&initial);
    ledger.assert_resource_opened_once(&alice_key).unwrap();
    ledger.assert_resource_opened_once(&bob_key).unwrap();
    ledger.assert_all_resources_have_owner().unwrap();
    ledger.assert_no_wildcard_resource_opened().unwrap();

    let shrink = set_sources(&mut target, sources(&["alice"]));
    ledger.apply_result(&shrink);
    ledger.assert_resource_not_open(&bob_key).unwrap();
    ledger.assert_resource_closed_once(&bob_key).unwrap();
    ledger.assert_no_duplicate_close().unwrap();
    ledger.assert_no_wildcard_resource_opened().unwrap();

    let empty = set_sources(&mut target, BTreeSet::new());
    ledger.apply_result(&empty);
    ledger.assert_resource_not_open(&alice_key).unwrap();
    ledger.assert_resource_not_open(&bob_key).unwrap();
    ledger.assert_resource_closed_once(&alice_key).unwrap();
    ledger.assert_all_resources_have_owner().unwrap();
    ledger.assert_no_duplicate_close().unwrap();
    ledger.assert_no_wildcard_resource_opened().unwrap();

    ledger
        .assert_command_order(&[
            ResourceCommandTrace {
                key: alice_key.clone(),
                scope: target.scope,
                kind: ResourceCommandKind::Open,
                transition: ResourceTransitionPolicy::Open,
            },
            ResourceCommandTrace {
                key: bob_key.clone(),
                scope: target.scope,
                kind: ResourceCommandKind::Open,
                transition: ResourceTransitionPolicy::Open,
            },
            ResourceCommandTrace {
                key: bob_key.clone(),
                scope: target.scope,
                kind: ResourceCommandKind::Close,
                transition: ResourceTransitionPolicy::Close,
            },
            ResourceCommandTrace {
                key: alice_key.clone(),
                scope: target.scope,
                kind: ResourceCommandKind::Close,
                transition: ResourceTransitionPolicy::Close,
            },
        ])
        .unwrap();

    assert!(target.graph.resource_owners(&alice_key).is_none());
    assert!(target.graph.resource_owners(&bob_key).is_none());
}

#[test]
fn host_status_feedback_cannot_resurrect_closed_demand() {
    let (mut target, initial) = build_graph(sources(&["alice"]));
    let alice = interest("alice");
    let alice_key = resource_key(&alice);
    let mut ledger = ResourceLedger::new();
    let mut host = FakeHost::new();

    let opened = host.apply_result(&mut ledger, &initial);
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].class, HostStatusClass::Current);
    let status_result = set_host_statuses(
        &mut target,
        opened.iter().map(|event| event.status.clone()).collect(),
    );
    assert!(status_result.resource_plan.commands().is_empty());

    let closed = close_scope(&mut target);
    let close_statuses = host.apply_result(&mut ledger, &closed);
    assert_eq!(close_statuses.len(), 1);
    assert_eq!(close_statuses[0].class, HostStatusClass::Current);
    ledger
        .assert_closed_scope_owns_no_resources(target.scope)
        .unwrap();

    let late_open = host.open_succeeds_later(
        &mut ledger,
        alice_key.clone(),
        target.scope,
        initial.revision,
    );
    assert_eq!(late_open.class, HostStatusClass::Late);
    let stale_open = host.observe_outcome(
        &mut ledger,
        alice_key.clone(),
        target.scope,
        trellis_core::Revision::new(0),
        HostResourceOutcome::Open,
    );
    assert_eq!(stale_open.class, HostStatusClass::Late);

    let late_status_result = set_host_statuses(
        &mut target,
        vec![close_statuses[0].status.clone(), late_open.status.clone()],
    );
    assert!(
        late_status_result.resource_plan.commands().is_empty(),
        "host feedback is canonical input, not demand"
    );
    ledger
        .assert_status_did_not_resurrect_closed_scope(target.scope)
        .unwrap();
    assert!(target.graph.resource_owners(&alice_key).is_none());
}

#[test]
fn materialized_rows_rebaseline_and_clear_without_hidden_shell_state() {
    let (mut target, initial) = build_graph(sources(&["alice"]));
    let output_key = target.output.key();
    let mut ledger = OutputLedger::new();

    ledger.apply_result(&initial);
    ledger
        .assert_current_equals(output_key, &rows_from_sources(&sources(&["alice"])))
        .unwrap();

    let expanded = set_sources(&mut target, sources(&["alice", "bob"]));
    ledger.apply_result(&expanded);
    let expected_rows = rows_from_sources(&sources(&["alice", "bob"]));
    ledger
        .assert_current_equals(output_key, &expected_rows)
        .unwrap();
    assert_incremental_equals_full::<_, OutputOracle>(
        &ledger,
        &(output_key, expected_rows.clone()),
    )
    .unwrap();

    let rebaseline = rebaseline_rows(&mut target);
    ledger.apply_result(&rebaseline);
    ledger.assert_revision_monotonic().unwrap();
    ledger
        .assert_delta_sequence_matches_rebaseline(output_key, &expected_rows)
        .unwrap();
    ledger
        .assert_consumer_needs_no_hidden_graph_state()
        .unwrap();
    assert!(matches!(
        &rebaseline.output_frames[0].kind,
        OutputFrameKind::Rebaseline(rows, _) if rows == &expected_rows
    ));

    let closed = close_scope(&mut target);
    ledger.close_scope(target.scope);
    ledger.apply_result(&closed);
    ledger.assert_cleared(output_key).unwrap();
    ledger.assert_closed_scope_cleared(target.scope).unwrap();
    ledger
        .assert_no_frame_for_closed_scope_except_terminal()
        .unwrap();
}

#[test]
fn scenario_replay_and_audit_traces_are_deterministic() {
    let first = read_session_scenario();
    let second = read_session_scenario();
    first.assert_replay_matches(&second).unwrap();
    assert_eq!(
        first.to_redacted_debug_string(&NoRedaction),
        second.to_redacted_debug_string(&NoRedaction)
    );
    first
        .assert_step_resource_commands(
            "source-shrink",
            &first.step("source-shrink").unwrap().trace.resource_commands,
        )
        .unwrap();

    let (target, initial) = build_graph(sources(&["alice"]));
    assert_no_unexplained_plan(&target.graph, &initial).unwrap();
    assert_every_resource_command_has_cause(&target.graph, &initial).unwrap();
    assert_no_unexplained_output_frame(&target.graph, &initial).unwrap();
    assert_every_output_frame_has_revision(&target.graph, &initial).unwrap();
    assert_every_output_frame_has_scope(&target.graph, &initial).unwrap();
    assert_dependency_path_exists(&target.graph, target.sources.id(), target.demand.id()).unwrap();
}

#[test]
fn conformance_report_names_the_remaining_nmp_adoption_gaps() {
    let report = ConformanceSuite::all().report(&[
        ConformanceLevel::DeterministicTrace,
        ConformanceLevel::ScopeResourceLifecycle,
        ConformanceLevel::MaterializedOutput,
        ConformanceLevel::FullRecomputeOracle,
    ]);

    assert!(report.supports(ConformanceLevel::DeterministicTrace));
    assert!(report.supports(ConformanceLevel::ScopeResourceLifecycle));
    assert!(report.supports(ConformanceLevel::MaterializedOutput));
    assert!(report.supports(ConformanceLevel::FullRecomputeOracle));
    assert!(report
        .unsupported_levels()
        .contains(&ConformanceLevel::GeneratedModelSequences));
    assert!(report
        .unsupported_levels()
        .contains(&ConformanceLevel::PerformanceSmoke));
}
