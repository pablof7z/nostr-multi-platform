use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{KernelEvent, ObservedProjection, ObservedProjectionRegistrar};
use nmp_core::{ObservedProjectionId, ObservedProjectionSink};
use nmp_nip25::{decode_reaction_aggregate_snapshot, ReactionAggregateProjection};
use nmp_planner::InterestShape;

use super::*;

#[derive(Clone)]
struct OpenRecord {
    id: ObservedProjectionId,
    consumer_id: String,
    shape: InterestShape,
    observer: Arc<dyn ObservedProjectionSink>,
}

#[derive(Default)]
struct RecordingRegistrar {
    next_id: Mutex<u64>,
    opened: Mutex<Vec<OpenRecord>>,
    closed: Mutex<Vec<ObservedProjectionId>>,
}

impl RecordingRegistrar {
    fn opened(&self) -> Vec<OpenRecord> {
        self.opened.lock().unwrap().clone()
    }

    fn closed(&self) -> Vec<ObservedProjectionId> {
        self.closed.lock().unwrap().clone()
    }
}

impl ObservedProjectionRegistrar for RecordingRegistrar {
    fn open_observed_projection(&self, decl: ObservedProjection) -> ObservedProjectionId {
        let mut next = self.next_id.lock().unwrap();
        *next += 1;
        let id = ObservedProjectionId(*next);
        self.opened.lock().unwrap().push(OpenRecord {
            id,
            consumer_id: decl.consumer_id,
            shape: decl.replay_shapes.into_iter().next().expect("shape"),
            observer: decl.observer,
        });
        id
    }

    fn close_observed_projection(&self, id: ObservedProjectionId) {
        self.closed.lock().unwrap().push(id);
    }

    fn observed_projection_registrar_handle(
        &self,
    ) -> Arc<dyn ObservedProjectionRegistrar + Send + Sync> {
        panic!("test does not request nested registrar handles")
    }
}

#[test]
fn reaction_observer_opens_and_closes_delete_projection_from_reaction_set() {
    let target = "a".repeat(64);
    let reactor = "b".repeat(64);
    let projection = Arc::new(ReactionAggregateProjection::new(Some(reactor.clone())));
    let observer = Arc::new(ReactionReadObserver::new(
        target.clone(),
        Arc::clone(&projection),
    ));
    let registrar = Arc::new(RecordingRegistrar::default());
    let reconciler = ObservedProjectionReconciler::new(
        registrar.clone(),
        observer.clone(),
        "test.reactions.deletes",
        SCOPE_GLOBAL,
        DEFAULT_FEED_WINDOW_LIMIT,
        {
            let projection = Arc::clone(&projection);
            let target = target.clone();
            Arc::new(move || delete_shape(&projection, &target))
        },
    );
    observer.set_delete_reconciler(reconciler);

    let reaction_id = "c".repeat(64);
    observer.on_kernel_event(&reaction(&reaction_id, &reactor, &target, "+"));

    let opened = registrar.opened();
    assert_eq!(opened.len(), 1, "reaction id opens a delete observer");
    assert!(opened[0].consumer_id.ends_with(".deletes"));
    assert_eq!(
        opened[0].shape.kinds,
        BTreeSet::from([KIND_REACTION_DELETE])
    );
    assert_eq!(opened[0].shape.authors, BTreeSet::from([reactor.clone()]));
    assert_eq!(
        opened[0].shape.tags.get("e").cloned().unwrap_or_default(),
        BTreeSet::from([reaction_id.clone()])
    );

    opened[0]
        .observer
        .on_kernel_event(&delete("d".repeat(64).as_str(), &reactor, &reaction_id));

    assert!(
        projection.aggregate_for(&target).is_none(),
        "kind:5 from the original reactor retracts the reaction"
    );
    assert_eq!(
        registrar.closed(),
        vec![opened[0].id],
        "empty reaction set closes the delete observer"
    );
}

#[test]
fn open_reactions_registers_typed_sidecar_and_close_is_stale_handle_safe() {
    let app = crate::new_app();
    let target = "d".repeat(64);
    let (first, reader) = app
        .open_reactions_with_reader(target.clone())
        .expect("valid target opens");
    assert_eq!(
        first.projection_key(),
        format!("nmp.nip25.reactions.{target}")
    );
    assert_eq!(reaction_session_count(&app), 1);
    assert_eq!(app.test_observed_projection_sink_count(), 1);
    assert!(
        app.registered_typed_projection_keys()
            .contains(&first.projection_key().to_string()),
        "typed sidecar is registered under the handle projection key"
    );

    reader.on_kernel_event(&reaction(
        "e".repeat(64).as_str(),
        &"f".repeat(64),
        &target,
        "+",
    ));
    let typed = app.run_typed_snapshot_projections_for_test();
    let row = typed
        .iter()
        .find(|row| row.key == first.projection_key())
        .expect("typed reaction sidecar emitted");
    let snapshot = decode_reaction_aggregate_snapshot(&row.payload).expect("decodes");
    assert_eq!(snapshot.targets.len(), 1);
    assert_eq!(snapshot.targets[0].target_event_id, target);
    assert_eq!(snapshot.targets[0].total, 1);

    let second = app
        .open_reactions(target.clone())
        .expect("replacement opens");
    assert_eq!(reaction_session_count(&app), 1);
    assert_eq!(
        app.test_observed_projection_sink_count(),
        1,
        "replacement closes the old observer before opening the new one"
    );

    app.close_reactions(first);
    assert_eq!(
        reaction_session_count(&app),
        1,
        "stale handle must not close the replacement"
    );
    app.close_reactions(second.clone());
    assert_eq!(reaction_session_count(&app), 0);
    assert_eq!(app.test_observed_projection_sink_count(), 0);
    assert!(
        !app.registered_typed_projection_keys()
            .contains(&second.projection_key().to_string()),
        "close removes the typed sidecar"
    );
}

#[test]
fn open_reactions_rejects_malformed_target() {
    let app = crate::new_app();
    assert!(app.open_reactions("not-a-hex-id").is_none());
    assert_eq!(reaction_session_count(&app), 0);
    assert_eq!(app.test_observed_projection_sink_count(), 0);
}

fn reaction(id: &str, author: &str, target: &str, token: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: KIND_REACTION,
        created_at: 100,
        tags: vec![vec!["e".to_string(), target.to_string()]],
        content: token.to_string(),
        relay_provenance: Vec::new(),
    }
}

fn delete(id: &str, author: &str, deleted_id: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: KIND_REACTION_DELETE,
        created_at: 101,
        tags: vec![vec!["e".to_string(), deleted_id.to_string()]],
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn reaction_session_count(app: &NmpApp) -> usize {
    app.reaction_read_sessions
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .len()
}
