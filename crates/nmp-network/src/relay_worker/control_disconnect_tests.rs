//! Control-channel disconnect lifecycle tests.
//!
//! Split out of `tests.rs` per the file-size gate. Pins that tearing down the
//! sender side without an explicit Shutdown still terminates the relay slot.

use std::sync::mpsc;
use std::time::Duration;

use super::tests::support::{drain_until, LocalServer, ServerObserved};
use super::{spawn_relay_worker_with_keepalive, RelayCommand, RelayEvent};
use crate::role::RelayRole;

/// When the control sender is dropped mid-session, the worker must emit a
/// terminal `RelayEvent::Closed` so consumers tracking slot health are not left
/// at the last non-terminal event.
#[test]
fn disconnected_control_sender_emits_terminal_closed_event() {
    let server = LocalServer::start_auto_pong();

    let (relay_tx, relay_rx) = mpsc::channel::<RelayEvent>();
    let control_tx = spawn_relay_worker_with_keepalive(
        RelayRole::Content,
        server.url.clone(),
        42,
        relay_tx,
        Duration::from_secs(30),
        Duration::from_secs(30),
        None,
    );

    let connected = drain_until(
        &relay_rx,
        |ev| matches!(ev, RelayEvent::Connected { .. }),
        Duration::from_secs(5),
    );
    assert!(
        connected.is_some(),
        "worker must report Connected before test can proceed"
    );

    // `Connected` is emitted before the connected-loop poller installs its
    // control-channel waker. This sentinel proves the worker processed a
    // post-connect control command before the sender is dropped.
    let sentinel = "control-drop-sentinel".to_string();
    control_tx
        .send(RelayCommand::Send(sentinel.clone()))
        .expect("sentinel send must be accepted by the worker channel");
    assert!(
        server.await_event(ServerObserved::Text(sentinel), Duration::from_secs(5)),
        "worker must drain a post-connect control command before sender drop"
    );

    drop(control_tx);

    let terminal = drain_until(
        &relay_rx,
        |ev| {
            matches!(
                ev,
                RelayEvent::Closed { generation: 42, .. }
                    | RelayEvent::Failed { generation: 42, .. }
            )
        },
        Duration::from_secs(5),
    );
    assert!(
        terminal.is_some(),
        "dropping the control sender must produce a terminal RelayEvent::Closed"
    );
    assert!(
        matches!(terminal.unwrap(), RelayEvent::Closed { .. }),
        "control-sender drop is a clean teardown; event must be Closed, not Failed"
    );
}
