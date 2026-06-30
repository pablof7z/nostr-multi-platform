//! Composition tests for the pointer-source controller: pointer ingest →
//! dependent target acquisition + delivery projection, driven through a
//! recording registrar and a capturing command sender.

use std::collections::BTreeSet;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

use nmp_core::actor::{ActorCommand, ActorMail, InterestsCommand};
use nmp_core::subs::SubOwnerKey;
use nmp_core::substrate::{KernelEvent, ObservedProjection, ObservedProjectionRegistrar};
use nmp_core::ObservedProjectionSink;
use nmp_core::{CommandSender, DependentInterestChild, ObservedProjectionId};
use nmp_planner::{InterestShape, NaddrCoord};

use super::{open_pointer_source, PointerSortMode, PointerSourceParams, PointerSourceSession};

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

    fn observer_for(&self, suffix: &str) -> Arc<dyn ObservedProjectionSink> {
        self.opened()
            .into_iter()
            .find(|record| record.consumer_id.ends_with(suffix))
            .unwrap_or_else(|| panic!("no projection opened for `{suffix}`"))
            .observer
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

struct Harness {
    session: PointerSourceSession,
    registrar: Arc<RecordingRegistrar>,
    rx: Receiver<ActorMail>,
}

fn open(sort: PointerSortMode) -> Harness {
    let registrar = Arc::new(RecordingRegistrar::default());
    let (tx, rx) = std::sync::mpsc::channel::<ActorMail>();
    let sender = CommandSender::new(tx);
    let session = open_pointer_source(
        sender,
        registrar.clone(),
        PointerSourceParams {
            pointer_shape: InterestShape {
                kinds: BTreeSet::from([6]),
                ..InterestShape::default()
            },
            consumer_id: "test.ps".to_string(),
            scope: 1,
            sort,
            replay_limit: 64,
            on_update: None,
        },
    );
    Harness {
        session,
        registrar,
        rx,
    }
}

fn pointer(id: &str, author: &str, created_at: u64, tags: Vec<Vec<&str>>) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 6,
        created_at,
        tags: tags
            .into_iter()
            .map(|tag| tag.into_iter().map(str::to_string).collect())
            .collect(),
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn target(id: &str, author: &str, kind: u32, created_at: u64, d: Option<&str>) -> KernelEvent {
    let mut tags = Vec::new();
    if let Some(d) = d {
        tags.push(vec!["d".to_string(), d.to_string()]);
    }
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind,
        created_at,
        tags,
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

type Replacement = (SubOwnerKey, Vec<DependentInterestChild>, String);

fn drain_replacements(rx: &Receiver<ActorMail>) -> Vec<Replacement> {
    let mut out = Vec::new();
    loop {
        match rx.try_recv() {
            Ok(ActorMail::Command(ActorCommand::Interests(
                InterestsCommand::ReplaceDependentInterestSet {
                    owner,
                    children,
                    reason,
                },
            ))) => out.push((owner, children, reason)),
            Ok(_) => {}
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
        }
    }
    out
}

#[test]
fn pointer_event_id_materializes_event_id_acquisition_and_delivery() {
    let h = open(PointerSortMode::Time);
    // The pointer source opens first, scoped to the declared pointer shape.
    let first = &h.registrar.opened()[0];
    assert!(first.consumer_id.ends_with(".pointer"));
    assert_eq!(first.shape.kinds, BTreeSet::from([6]));

    h.registrar
        .observer_for(".pointer")
        .on_kernel_event(&pointer("p1", "alice", 100, vec![vec!["e", "noteX"]]));

    // Exactly one dependent child, an `event_ids` predicate for the target.
    let replacements = drain_replacements(&h.rx);
    assert_eq!(replacements.len(), 1);
    let (_, children, _) = &replacements[0];
    assert_eq!(children.len(), 1);
    assert_eq!(
        children[0].interest.shape.event_ids,
        BTreeSet::from(["noteX".to_string()])
    );
    assert!(children[0].interest.shape.addresses.is_empty());

    // Delivery projection opens over the same event-id union shape.
    let delivery = h
        .registrar
        .opened()
        .into_iter()
        .find(|r| r.consumer_id.ends_with(".target"))
        .expect("delivery projection opened");
    assert_eq!(
        delivery.shape.event_ids,
        BTreeSet::from(["noteX".to_string()])
    );

    // A delivered target hydrates the projection.
    h.registrar
        .observer_for(".target")
        .on_kernel_event(&target("noteX", "carol", 1, 90, None));
    let items = h.session.model().lock().unwrap().items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].event.id, "noteX");
    assert_eq!(items[0].pointer_count, 1);
}

#[test]
fn pointer_address_materializes_address_acquisition() {
    let h = open(PointerSortMode::Time);
    h.registrar
        .observer_for(".pointer")
        .on_kernel_event(&pointer(
            "p1",
            "alice",
            100,
            vec![vec!["a", "30023:bob:slug"]],
        ));

    let replacements = drain_replacements(&h.rx);
    assert_eq!(replacements.len(), 1);
    let (_, children, _) = &replacements[0];
    assert_eq!(children.len(), 1);
    let coord = NaddrCoord {
        pubkey: "bob".to_string(),
        kind: 30_023,
        d_tag: "slug".to_string(),
    };
    assert_eq!(
        children[0].interest.shape.addresses,
        BTreeSet::from([coord.clone()])
    );
    assert_eq!(
        children[0].interest.shape.kinds,
        BTreeSet::from([30_023]),
        "addressable kind must travel with the coordinate for cache-serve"
    );
    assert!(children[0].interest.shape.event_ids.is_empty());

    let delivery = h.registrar.observer_for(".target");
    delivery.on_kernel_event(&target("v1", "bob", 30_023, 80, Some("slug")));
    let items = h.session.model().lock().unwrap().items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].event.id, "v1");
}

#[test]
fn cross_consumer_targets_share_one_dependent_child_key() {
    // Two independent read models that point at the same target produce children
    // with the SAME SubKey, so the kernel registry collapses them onto one slot.
    let a = open(PointerSortMode::Time);
    let b = open(PointerSortMode::Time);
    a.registrar
        .observer_for(".pointer")
        .on_kernel_event(&pointer("pa", "alice", 1, vec![vec!["e", "shared"]]));
    b.registrar
        .observer_for(".pointer")
        .on_kernel_event(&pointer("pb", "bob", 2, vec![vec!["e", "shared"]]));

    let child_a = drain_replacements(&a.rx).remove(0).1.remove(0);
    let child_b = drain_replacements(&b.rx).remove(0).1.remove(0);
    assert_eq!(child_a.key, child_b.key);
    assert_eq!(child_a.scope, child_b.scope);
}

#[test]
fn empty_reduction_fails_closed() {
    let h = open(PointerSortMode::Time);
    let opened_before = h.registrar.opened().len();
    // A pointer with no `e` / `a` reference must not widen demand.
    h.registrar
        .observer_for(".pointer")
        .on_kernel_event(&pointer("p1", "alice", 100, vec![vec!["p", "carol"]]));

    assert!(
        drain_replacements(&h.rx).is_empty(),
        "no target → no acquisition command"
    );
    assert_eq!(
        h.registrar.opened().len(),
        opened_before,
        "no target → no delivery projection (no wildcard query)"
    );
}

#[test]
fn sort_change_does_not_reopen_or_reacquire() {
    let h = open(PointerSortMode::Time);
    h.registrar
        .observer_for(".pointer")
        .on_kernel_event(&pointer("p1", "alice", 100, vec![vec!["e", "noteX"]]));
    let _ = drain_replacements(&h.rx);
    let opened_before = h.registrar.opened().len();
    let closed_before = h.registrar.closed().len();

    h.session.set_sort(PointerSortMode::Count);

    assert!(
        drain_replacements(&h.rx).is_empty(),
        "sort is read-model state — no acquisition replacement"
    );
    assert_eq!(h.registrar.opened().len(), opened_before, "no reopen");
    assert_eq!(h.registrar.closed().len(), closed_before, "no close");
}

#[test]
fn close_releases_pointer_delivery_and_target_set() {
    let h = open(PointerSortMode::Time);
    h.registrar
        .observer_for(".pointer")
        .on_kernel_event(&pointer("p1", "alice", 100, vec![vec!["e", "noteX"]]));
    let _ = drain_replacements(&h.rx);

    let pointer_id = h.registrar.opened()[0].id;
    let delivery_id = h
        .registrar
        .opened()
        .into_iter()
        .find(|r| r.consumer_id.ends_with(".target"))
        .expect("delivery opened")
        .id;

    h.session.close();

    let closed = h.registrar.closed();
    assert!(closed.contains(&pointer_id), "pointer projection closed");
    assert!(closed.contains(&delivery_id), "delivery projection closed");
    // The dependent target set is cleared with an empty replacement.
    let replacements = drain_replacements(&h.rx);
    assert_eq!(replacements.len(), 1);
    assert!(replacements[0].1.is_empty(), "target set cleared on close");
}
