//! Tests for the dispatcher integration.
//!
//! The dispatcher is wired directly into the persistence chokepoint and
//! receives typed `RawEvent`s without a JSON-round-trip.  Policy registration
//! installs `ExternalEventSinkPolicy` objects on the dispatcher directly.

use std::sync::{Arc, Mutex};

use crate::actor::raw_event_forwarder::register_raw_event_forward_policies;
use crate::kernel::Kernel;
use crate::store::RawEvent;
use crate::substrate::external_event_sink::{SignedEventFrame, SinkDestination};
use crate::substrate::{
    ExternalEventSinkDispatcher, ExternalEventSinkPolicy, RawEventForwardTarget,
};
use crate::{KindFilter, RelayRole};

// ─── Capture helpers ──────────────────────────────────────────────────────────

#[derive(Clone, Default)]
struct CapturePolicy {
    frames: Arc<Mutex<Vec<SignedEventFrame>>>,
}

impl CapturePolicy {
    fn frames(&self) -> Vec<SignedEventFrame> {
        self.frames.lock().expect("frames").clone()
    }
}

impl ExternalEventSinkPolicy for CapturePolicy {
    fn kind_filter(&self) -> KindFilter {
        KindFilter::from_kinds([0u32])
    }

    fn destinations(&self, frame: &SignedEventFrame) -> Vec<SinkDestination> {
        self.frames.lock().expect("frames").push(frame.clone());
        vec![] // no relay delivery in unit tests
    }
}

/// A static policy that always returns a single relay target — used to
/// exercise the `ExternalEventSinkPolicySlot` registration path.
struct StaticPolicy {
    target: String,
}

impl ExternalEventSinkPolicy for StaticPolicy {
    fn kind_filter(&self) -> KindFilter {
        KindFilter::from_kinds([0u32])
    }

    fn destinations(&self, _frame: &SignedEventFrame) -> Vec<SinkDestination> {
        vec![SinkDestination::Relay(RawEventForwardTarget::new(
            self.target.clone(),
            RelayRole::Indexer,
        ))]
    }
}

fn make_pool() -> nmp_network::pool::Pool {
    let (relay_tx, _relay_rx) = std::sync::mpsc::channel();
    nmp_network::pool::Pool::new(nmp_network::pool::PoolConfig::default(), relay_tx)
}

fn make_raw(kind: u32) -> RawEvent {
    RawEvent {
        id: "01".repeat(32),
        pubkey: "11".repeat(32),
        created_at: 1_700_000_000,
        kind,
        tags: Vec::new(),
        content: String::new(),
        sig: "22".repeat(64),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn dispatcher_accepts_frames_from_capture_policy() {
    let dispatcher = ExternalEventSinkDispatcher::new();
    dispatcher.bind_runtime(make_pool());

    let capture = Arc::new(CapturePolicy::default());
    dispatcher.set_policies(vec![capture.clone() as Arc<dyn ExternalEventSinkPolicy>]);

    // Build a frame and dispatch it.
    let raw = make_raw(0);
    use crate::substrate::external_event_sink::{IngestOutcomeKind, SignedEventFrame};
    let frame = SignedEventFrame::build(
        Arc::new(raw),
        Some(Arc::from("wss://relay/")),
        IngestOutcomeKind::Inserted,
    )
    .expect("frame");

    dispatcher.dispatch(frame);

    // Give the worker thread a moment to drain the channel.
    std::thread::sleep(std::time::Duration::from_millis(50));

    let frames = capture.frames();
    assert_eq!(frames.len(), 1, "expected exactly one frame delivered");
    let f = &frames[0];
    assert_eq!(f.raw.kind, 0);
    assert!(
        f.canonical_json.contains("\"kind\":0"),
        "canonical_json should contain kind:0"
    );
    assert_eq!(f.source_relay.as_deref(), Some("wss://relay/"));
}

/// `register_raw_event_forward_policies` installs policies on the dispatcher.
/// The dispatcher must be non-idle for the registered kind after installation.
#[test]
fn register_policies_installs_dispatcher_policies() {
    let sink_policy_slot = crate::slots::new_external_event_sink_policy_slot();

    {
        let mut guard = sink_policy_slot.lock().expect("policy slot");
        *guard = Some(Arc::new(|_context| {
            vec![Arc::new(StaticPolicy {
                target: "wss://indexer/".into(),
            }) as Arc<dyn ExternalEventSinkPolicy>]
        }));
    }

    let dispatcher = ExternalEventSinkDispatcher::new();
    let kernel = Kernel::new(crate::relay::DEFAULT_VISIBLE_LIMIT);

    register_raw_event_forward_policies(&kernel, &dispatcher, &sink_policy_slot);

    // The dispatcher handles kind:0 via its policy list.
    assert!(
        !dispatcher.all_idle_for_kind(0),
        "dispatcher must NOT be idle for kind 0 after registration"
    );
}
