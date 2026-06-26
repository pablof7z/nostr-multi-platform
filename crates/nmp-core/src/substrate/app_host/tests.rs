use super::*;
use crate::substrate::KernelEvent;
use crate::ObservedProjectionSink;
use nmp_planner::InterestShape;
use std::sync::Arc;

struct NoopObserver;

impl ObservedProjectionSink for NoopObserver {
    fn on_kernel_event(&self, _event: &KernelEvent) {}
}

fn noop_sink() -> Arc<dyn ObservedProjectionSink> {
    Arc::new(NoopObserver)
}

#[test]
fn observed_projection_from_kinds_declares_filter_and_replay_shape() {
    let decl = ObservedProjection::from_kinds(noop_sink(), "test.consumer", 1, [1, 3], 128);

    assert!(
        decl.has_declared_shape(),
        "kind-scoped read models must be accepted"
    );
    assert_eq!(decl.consumer_id, "test.consumer");
    assert_eq!(decl.scope, 1);
    assert_eq!(decl.replay_limit, 128);
    assert_eq!(decl.replay_shapes.len(), 1);
    assert!(decl.replay_shapes[0].kinds.contains(&1));
    assert!(decl.replay_shapes[0].kinds.contains(&3));
    assert!(
        decl.filter_json.contains("\"kinds\""),
        "wire filter must carry the declared shape"
    );
}

#[test]
fn observed_projection_from_shape_preserves_relay_pin() {
    let shape = InterestShape {
        relay_pin: Some("wss://relay.example".to_string()),
        kinds: [1].into_iter().collect(),
        ..Default::default()
    };

    let decl = ObservedProjection::from_shape(noop_sink(), "test.pinned", 1, shape, 32);

    assert!(
        decl.has_declared_shape(),
        "relay-pinned declarations still need an event predicate"
    );
    assert_eq!(decl.relay_pin.as_deref(), Some("wss://relay.example"));
    assert_eq!(
        decl.replay_shapes[0].relay_pin.as_deref(),
        Some("wss://relay.example")
    );
}

#[test]
fn filterless_observed_projection_is_rejected() {
    let decl = ObservedProjection {
        observer: noop_sink(),
        filter_json: "{}".to_string(),
        consumer_id: "test.filterless".to_string(),
        scope: 1,
        relay_pin: None,
        replay_shapes: vec![InterestShape::default()],
        replay_limit: 32,
    };

    assert!(
        !decl.has_declared_shape(),
        "production read models must not recreate all-event observer fan-out"
    );
}
