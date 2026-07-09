//! Concept-side proofs for `open_threading_read_model` (#3096): the door
//! composes a caller-supplied [`InterestShape`] scope into a single routed
//! demand + reducer + typed output, then drives it through the ONE engine
//! (`nmp-read-session`) — with no lifecycle code of its own (a fake host
//! records the engine calls, exactly like `nmp_reposts::open_reposts`'s own
//! proof).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{KernelEvent, ObservedProjection};
use nmp_core::{ObservedProjectionId, ObservedProjectionSink};
use nmp_ownership::ProjectionRegistrationKey;
use nmp_planner::InterestShape;
use nmp_read_session::{
    ReadHost, ReadInterestController, ReadOutputEncoder, ReadSessionBuild, ReadSessionId,
    ReadSessionRegistry, TeardownAction,
};

use super::*;

const AUTHOR: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ROOT: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const REPLY: &str = "2222222222222222222222222222222222222222222222222222222222222222";

fn event(id: &str, author: &str, tags: Vec<Vec<&str>>) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 1,
        created_at: 1,
        tags: tags
            .into_iter()
            .map(|tag| tag.into_iter().map(str::to_string).collect())
            .collect(),
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn params() -> ThreadingReadModelParams {
    ThreadingReadModelParams::global(
        "session-a",
        InterestShape {
            kinds: [1].into_iter().collect(),
            ..Default::default()
        },
    )
}

#[derive(Default)]
struct FakeHost {
    registry: ReadSessionRegistry,
    observers: Arc<Mutex<Vec<Arc<dyn ObservedProjectionSink>>>>,
    encoder: Mutex<Option<ReadOutputEncoder>>,
    output_key: Arc<Mutex<Option<String>>>,
    opened_filters: Arc<Mutex<Vec<String>>>,
    closed_interests: Arc<Mutex<Vec<u64>>>,
    next_interest: Arc<AtomicU64>,
}

impl FakeHost {
    fn run_encoder(&self) -> Option<nmp_core::TypedProjectionData> {
        self.encoder.lock().unwrap().as_ref().and_then(|e| e())
    }
    fn feed(&self, event: &KernelEvent) {
        let observer = self.observers.lock().unwrap().first().cloned();
        if let Some(obs) = observer {
            obs.on_kernel_event(event);
        }
    }
}

impl ReadHost for FakeHost {
    fn install_read_output(&self, key: ProjectionRegistrationKey, encoder: ReadOutputEncoder) {
        *self.output_key.lock().unwrap() = Some(key.as_str().to_string());
        *self.encoder.lock().unwrap() = Some(encoder);
    }
    fn open_read_interest(&self, decl: ObservedProjection) -> ObservedProjectionId {
        self.observers
            .lock()
            .unwrap()
            .push(Arc::clone(&decl.observer));
        self.opened_filters.lock().unwrap().push(decl.filter_json);
        ObservedProjectionId(self.next_interest.fetch_add(1, Ordering::Relaxed) + 1)
    }
    fn teardown_close_interest(&self, id: ObservedProjectionId) -> TeardownAction {
        let closed = Arc::clone(&self.closed_interests);
        Box::new(move || closed.lock().unwrap().push(id.0))
    }
    fn teardown_remove_output(&self, _key: String) -> TeardownAction {
        let output = Arc::clone(&self.output_key);
        Box::new(move || *output.lock().unwrap() = None)
    }
    fn teardown_mark_changed(&self) -> TeardownAction {
        Box::new(|| {})
    }
    fn store_read_session(&self, build: ReadSessionBuild) -> ReadSessionId {
        self.registry.open(build)
    }
    fn read_session_projection_key(&self, id: &ReadSessionId) -> Option<String> {
        self.registry.projection_key(id)
    }
    fn close_read_session(&self, id: &ReadSessionId) -> bool {
        self.registry.close(id)
    }
    fn close_read_session_by_projection_key(&self, projection_key: &str) -> bool {
        self.registry.close_by_projection_key(projection_key)
    }
    fn read_interest_controller(&self) -> Option<ReadInterestController> {
        // Threading opens no dependent demand, but every other concept-read
        // supplies this hook, so the fake host does too for parity.
        let observers = Arc::clone(&self.observers);
        let opened_filters = Arc::clone(&self.opened_filters);
        let next_interest = Arc::clone(&self.next_interest);
        let open = move |decl: ObservedProjection| {
            observers.lock().unwrap().push(Arc::clone(&decl.observer));
            opened_filters.lock().unwrap().push(decl.filter_json);
            ObservedProjectionId(next_interest.fetch_add(1, Ordering::Relaxed) + 1)
        };
        let closed = Arc::clone(&self.closed_interests);
        let close = move |id: ObservedProjectionId| {
            closed.lock().unwrap().push(id.0);
        };
        Some(ReadInterestController::new(open, close))
    }
}

#[test]
fn threading_projection_key_validates_the_session_suffix() {
    assert_eq!(
        threading_projection_key("session-a"),
        Some("nmp.threading.graph.session-a".to_string())
    );
    assert_eq!(threading_projection_key(""), None);
    assert_eq!(threading_projection_key("has space"), None);
    assert_eq!(
        threading_projection_key(&"x".repeat(THREADING_GRAPH_SESSION_ID_MAX_LEN + 1)),
        None
    );
}

#[test]
fn open_rejects_an_invalid_session_id() {
    let host = FakeHost::default();
    let mut bad = params();
    bad.session_id = "has space".to_string();
    assert!(open_threading_read_model(&host, bad).is_none());
    assert_eq!(host.registry.live_count(), 0);
}

#[test]
fn open_threading_read_model_drives_the_engine_and_close_withdraws_everything() {
    let host = FakeHost::default();
    let handle = open_threading_read_model(&host, params()).expect("valid session id opens");

    assert_eq!(
        handle.projection_key(),
        "nmp.threading.graph.session-a",
        "framework-owned per-session output key"
    );
    assert_eq!(
        host.opened_filters.lock().unwrap().len(),
        1,
        "one demand opened for the caller-supplied shape"
    );
    assert!(
        host.opened_filters.lock().unwrap()[0].contains(r#""kinds":[1]"#),
        "the compiled filter carries the caller's shape: {:?}",
        host.opened_filters.lock().unwrap()
    );
    assert_eq!(
        host.registry.live_count(),
        1,
        "the read lands in the ONE shared registry (#3096 leak audit)"
    );
    assert_eq!(
        host.output_key.lock().unwrap().as_deref(),
        Some(handle.projection_key()),
        "typed output installed under the handle's key"
    );

    // Live delivery folds into the typed output the shell renders.
    host.feed(&event(REPLY, AUTHOR, vec![vec!["e", ROOT]]));
    let data = host.run_encoder().expect("output emits");
    let decoded = crate::decode_threading_snapshot(&data.payload).unwrap();
    assert_eq!(decoded.edges.len(), 1);
    assert_eq!(decoded.edges[0].event_id, REPLY);

    // Close withdraws the demand and tombstones the output — reverse order,
    // once — and the engine no longer tracks the read (no leak).
    assert!(close_threading_read_model(&host, handle));
    assert_eq!(
        host.closed_interests.lock().unwrap().len(),
        1,
        "the primary demand is withdrawn"
    );
    assert!(
        host.output_key.lock().unwrap().is_none(),
        "output tombstoned"
    );
    assert_eq!(
        host.registry.live_count(),
        0,
        "no leak after close (#3096 leak audit)"
    );
}

#[test]
fn close_is_idempotent_on_an_already_closed_handle() {
    let host = FakeHost::default();
    let handle = open_threading_read_model(&host, params()).unwrap();
    assert!(close_threading_read_model(&host, handle.clone()));
    assert!(
        !close_threading_read_model(&host, handle),
        "closing again is a safe no-op (D6)"
    );
}
