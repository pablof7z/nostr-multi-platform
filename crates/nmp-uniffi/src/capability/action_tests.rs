//! Action-lane UniFFI tests.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use nmp_core::actor::ActorCommand;
use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};

use super::ActionResultObserver;
use crate::NmpApp;

struct RecordObserver {
    received: Arc<Mutex<Vec<String>>>,
}

impl RecordObserver {
    fn new_boxed() -> (Box<dyn ActionResultObserver>, Arc<Mutex<Vec<String>>>) {
        let received = Arc::new(Mutex::new(Vec::new()));
        let handle = Arc::clone(&received);
        (Box::new(RecordObserver { received }), handle)
    }
}

impl ActionResultObserver for RecordObserver {
    fn on_action_result(&self, result_json: String) {
        self.received.lock().unwrap().push(result_json);
    }
}

struct BlockingActionResultObserver {
    entered_tx: Mutex<Option<mpsc::Sender<()>>>,
    gate: Arc<std::sync::Barrier>,
}

impl ActionResultObserver for BlockingActionResultObserver {
    fn on_action_result(&self, _result_json: String) {
        if let Ok(mut guard) = self.entered_tx.lock() {
            let _ = guard.take().map(|tx| tx.send(()));
        }
        self.gate.wait();
    }
}

struct SucceedModule; // doctrine-allow: action_namespace — test-only namespace inside #[cfg(test)]

impl ActionModule for SucceedModule {
    const NAMESPACE: nmp_core::substrate::DeclaredActionNamespace =
        nmp_core::substrate::DeclaredActionNamespace::app_owned("test.uniffi_c4.succeed");
    type Action = serde_json::Value;

    fn decode_payload(
        _bytes: &[u8],
    ) -> Option<Result<Self::Action, nmp_core::substrate::ActionPayloadDecodeError>> {
        Some(Ok(serde_json::Value::Null))
    }

    fn start(
        &self,
        _ctx: &mut ActionContext,
        _action: Self::Action,
    ) -> Result<(), ActionRejection> {
        Ok(())
    }

    fn execute(
        &self,
        _ctx: &nmp_core::substrate::ActionContext,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn action_result_observer_fires_on_dispatch() {
    let mut inner = nmp_native_runtime::new_app();
    let _ = inner.register_action(SucceedModule);
    let app = std::sync::Arc::new(NmpApp {
        inner,
        search_handles: Default::default(),
    });

    let (observer, received) = RecordObserver::new_boxed();
    app.register_action_result_observer(observer);

    let envelope = encode_dispatch_envelope(
        "corr-obs-1",
        SucceedModule::NAMESPACE,
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &[0u8; 4],
    );
    let outcome = app.dispatch_action(envelope);
    assert!(outcome.correlation_id.is_some(), "dispatch must succeed");

    let calls = received.lock().unwrap();
    assert_eq!(calls.len(), 1, "observer must fire exactly once");
    let v: serde_json::Value = serde_json::from_str(&calls[0]).unwrap();
    assert_eq!(
        v["correlation_id"].as_str(),
        Some("corr-obs-1"),
        "observer must carry the correlation_id",
    );
}

#[test]
fn action_result_observer_replace_is_safe() {
    let mut inner = nmp_native_runtime::new_app();
    let _ = inner.register_action(SucceedModule);
    let app = std::sync::Arc::new(NmpApp {
        inner,
        search_handles: Default::default(),
    });

    let (observer_a, received_a) = RecordObserver::new_boxed();
    app.register_action_result_observer(observer_a);

    let env1 = encode_dispatch_envelope(
        "corr-obs-2a",
        SucceedModule::NAMESPACE,
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &[0u8; 4],
    );
    let _ = app.dispatch_action(env1);
    assert_eq!(
        received_a.lock().unwrap().len(),
        1,
        "observer A: first dispatch",
    );

    let (observer_b, received_b) = RecordObserver::new_boxed();
    app.register_action_result_observer(observer_b);

    let env2 = encode_dispatch_envelope(
        "corr-obs-2b",
        SucceedModule::NAMESPACE,
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &[0u8; 4],
    );
    let _ = app.dispatch_action(env2);
    assert_eq!(
        received_a.lock().unwrap().len(),
        1,
        "observer A must not fire after replacement",
    );
    assert_eq!(
        received_b.lock().unwrap().len(),
        1,
        "observer B: second dispatch",
    );
}

#[test]
fn action_result_observer_panic_is_contained() {
    struct PanickingObserver;
    impl ActionResultObserver for PanickingObserver {
        fn on_action_result(&self, _result_json: String) {
            panic!("PanickingObserver: deliberate panic");
        }
    }

    let mut inner = nmp_native_runtime::new_app();
    let _ = inner.register_action(SucceedModule);
    let app = std::sync::Arc::new(NmpApp {
        inner,
        search_handles: Default::default(),
    });

    app.register_action_result_observer(Box::new(PanickingObserver));

    let envelope = encode_dispatch_envelope(
        "corr-obs-panic",
        SucceedModule::NAMESPACE,
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &[0u8; 4],
    );
    let outcome = app.dispatch_action(envelope);
    assert!(
        outcome.correlation_id.is_some(),
        "dispatch must succeed even when observer panics",
    );
}

#[test]
fn action_result_observer_clear_waits_for_in_flight() {
    let mut inner = nmp_native_runtime::new_app();
    let _ = inner.register_action(SucceedModule);
    let app = std::sync::Arc::new(NmpApp {
        inner,
        search_handles: Default::default(),
    });
    let gate = Arc::new(std::sync::Barrier::new(2));
    let (entered_tx, entered_rx) = mpsc::channel::<()>();

    app.register_action_result_observer(Box::new(BlockingActionResultObserver {
        entered_tx: Mutex::new(Some(entered_tx)),
        gate: Arc::clone(&gate),
    }));

    let app_for_dispatch = Arc::clone(&app);
    let dispatch = thread::spawn(move || {
        let envelope = encode_dispatch_envelope(
            "corr-obs-clear",
            SucceedModule::NAMESPACE,
            DISPATCH_ENVELOPE_SCHEMA_VERSION,
            &[0u8; 4],
        );
        let outcome = app_for_dispatch.dispatch_action(envelope);
        assert!(outcome.correlation_id.is_some(), "dispatch must succeed");
    });
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("observer entered");

    let app_for_clear = Arc::clone(&app);
    let (clear_started_tx, clear_started_rx) = mpsc::channel::<()>();
    let (clear_done_tx, clear_done_rx) = mpsc::channel::<()>();
    let clear = thread::spawn(move || {
        clear_started_tx.send(()).unwrap();
        app_for_clear.clear_action_result_observer();
        clear_done_tx.send(()).unwrap();
    });
    clear_started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("clear started");
    assert!(
        clear_done_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "clear returned while action-result observer was in-flight",
    );

    gate.wait();
    dispatch.join().unwrap();
    clear.join().unwrap();
    clear_done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("clear returns after observer drains");
}
