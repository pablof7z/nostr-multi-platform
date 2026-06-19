//! ADR-0061 — the canonical feed surface over the web worker.
//!
//! `OpenFeed` replies with a deterministic key computed by the SAME
//! `nmp_feed::surface` canonicalization the native C-ABI uses, so the web key ==
//! the native key for a given descriptor. Requests are built from JSON (the wire
//! form the JS host sends) so these tests need no direct `nmp_feed` import.

use nmp_wasm::{WasmRuntime, WorkerEvent, WorkerRequest};
use serde_json::json;

fn open_feed_json(descriptor: serde_json::Value, correlation_id: &str) -> WorkerRequest {
    serde_json::from_value(json!({
        "type": "open_feed",
        "descriptor": descriptor,
        "correlation_id": correlation_id,
    }))
    .expect("OpenFeed request decodes")
}

#[test]
fn open_feed_returns_a_deterministic_key_independent_of_field_order() {
    let mut runtime = WasmRuntime::new();

    let canonical =
        json!({"profile":"notes","source":{"homeFollowSet":{}},"scope":"activeAccount"});
    let reordered =
        json!({"scope":"activeAccount","source":{"homeFollowSet":{}},"profile":"notes"});

    let key_of = |runtime: &mut WasmRuntime, descriptor: serde_json::Value| -> String {
        match runtime
            .handle(open_feed_json(descriptor, "open-1"))
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
        {
            WorkerEvent::FeedOpened { feed_key, .. } => feed_key,
            other => panic!("expected FeedOpened, got {other:?}"),
        }
    };

    let k1 = key_of(&mut runtime, canonical);
    let k2 = key_of(&mut runtime, reordered);
    assert!(k1.starts_with("nmp.feed."), "namespaced deterministic key");
    assert_eq!(k1, k2, "field order must not change the canonical key");
}

#[test]
fn set_feed_viewport_is_accepted_even_with_no_binding() {
    // Web composition binds no opener yet (shell-migration PR): the surface
    // honestly accepts the viewport report as a no-op rather than erroring.
    let mut runtime = WasmRuntime::new();
    let request: WorkerRequest = serde_json::from_value(json!({
        "type": "set_feed_viewport",
        "key": "nmp.feed.deadbeefdeadbeef",
        "viewport": {"firstVisible": 0, "lastVisible": 19, "renderedLen": 20},
        "correlation_id": "vp-1",
    }))
    .expect("SetFeedViewport decodes");
    let events = runtime.handle(request).unwrap();
    assert!(matches!(
        events.as_slice(),
        [WorkerEvent::ActionAccepted { .. }]
    ));
}
