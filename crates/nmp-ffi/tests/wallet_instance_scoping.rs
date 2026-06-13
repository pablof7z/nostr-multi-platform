//! K2 rung 5.2 ORACLE — two-instance wallet interop / no-crosstalk.
//!
//! ADR-0052 D1: `ActionModule` becomes value-registered; the NIP-47 wallet
//! modules own their `WalletRuntimeHandle` (no `ACTIVE_WALLET_RUNTIME`
//! process-global). The decisive falsification of the old design is: construct
//! TWO independent `NmpApp` instances in ONE process, give each its OWN wallet
//! runtime handle, register the wallet modules on each, dispatch a wallet
//! action on each, and assert each instance's module routes to ITS OWN handle —
//! never a shared global.
//!
//! Before the change this test cannot even be written (the modules are unit
//! structs reading a single `OnceLock`, so the second app silently rides the
//! first); after the change it passes because each module carries its handle
//! by value.
//!
//! Requires the `test-support` feature (exposes
//! [`NmpApp::action_registry_for_test`]). Run:
//! `cargo test -p nmp-ffi --features test-support --test wallet_instance_scoping`.
#![cfg(feature = "test-support")]

use std::sync::Arc;

use nmp_core::substrate::ActionModule;
use nmp_ffi::NmpApp;
use nmp_nip47::{
    new_wallet_runtime_handle, WalletConnectCommand, WalletConnectModule, WalletRuntimeHandle,
};

/// Drive a single app: register a `WalletConnectModule` carrying `handle`,
/// then dispatch `nmp.wallet.connect` and capture the emitted
/// `ActorCommand::Protocol(WalletConnectCommand)`. Return the
/// `WalletRuntimeHandle` the captured command carries, so the caller can
/// assert it is THIS app's handle and not the other app's.
fn register_and_capture_connect_handle(
    app: &mut NmpApp,
    handle: WalletRuntimeHandle,
    uri: &str,
) -> WalletRuntimeHandle {
    use std::cell::RefCell;

    // Value registration (D1): the module owns its handle.
    app.register_action(WalletConnectModule::new(handle));

    let action_json = serde_json::json!({ "Connect": { "uri": uri } }).to_string();
    let captured: RefCell<Vec<nmp_core::ActorCommand>> = RefCell::new(Vec::new());

    // Validate + execute through the app's OWN registry (the production path
    // `nmp_app_dispatch_action` takes), capturing the enqueued ActorCommand.
    let registry = app.action_registry_for_test();
    let cid = registry
        .start(
            &mut nmp_core::substrate::ActionContext::default(),
            0,
            WalletConnectModule::NAMESPACE,
            &action_json,
        )
        .expect("connect action must validate");
    registry
        .execute(
            WalletConnectModule::NAMESPACE,
            &action_json,
            &cid,
            &|cmd| captured.borrow_mut().push(cmd),
        )
        .expect("connect execute must succeed");

    let cmds = captured.into_inner();
    assert_eq!(cmds.len(), 1, "connect must emit exactly one ActorCommand");
    match cmds.into_iter().next().unwrap() {
        nmp_core::ActorCommand::Protocol(boxed) => {
            let connect = boxed
                .as_any()
                .downcast_ref::<WalletConnectCommand>()
                .expect("emitted command must be a WalletConnectCommand");
            assert_eq!(connect.uri, uri, "command must carry this app's URI");
            connect.runtime.clone()
        }
        other => panic!("expected ActorCommand::Protocol, got {other:?}"),
    }
}

/// THE ORACLE: two apps, two handles, zero crosstalk.
#[test]
fn two_apps_route_wallet_action_to_their_own_runtime_handle() {
    // SAFETY: `nmp_app_new` returns an owned heap `NmpApp`; we take exclusive
    // `&mut` references and free both at the end. No actor is started.
    let app_a_ptr = nmp_ffi::nmp_app_new();
    let app_b_ptr = nmp_ffi::nmp_app_new();
    assert!(!app_a_ptr.is_null() && !app_b_ptr.is_null());

    let handle_a: WalletRuntimeHandle = new_wallet_runtime_handle();
    let handle_b: WalletRuntimeHandle = new_wallet_runtime_handle();
    assert!(
        !Arc::ptr_eq(&handle_a, &handle_b),
        "test handles must be distinct Arcs"
    );

    let routed_a = {
        let app_a = unsafe { &mut *app_a_ptr };
        register_and_capture_connect_handle(
            app_a,
            Arc::clone(&handle_a),
            "nostr+walletconnect://aaaa?relay=wss://relay.a.example",
        )
    };
    let routed_b = {
        let app_b = unsafe { &mut *app_b_ptr };
        register_and_capture_connect_handle(
            app_b,
            Arc::clone(&handle_b),
            "nostr+walletconnect://bbbb?relay=wss://relay.b.example",
        )
    };

    // No crosstalk: each app's dispatched command carries ITS OWN handle.
    assert!(
        Arc::ptr_eq(&routed_a, &handle_a),
        "app A must route to handle A"
    );
    assert!(
        Arc::ptr_eq(&routed_b, &handle_b),
        "app B must route to handle B"
    );
    // The decisive anti-crosstalk assertion: A did not ride B's handle.
    assert!(
        !Arc::ptr_eq(&routed_a, &handle_b),
        "app A must NOT route to app B's handle (the old OnceLock crosstalk)"
    );
    assert!(
        !Arc::ptr_eq(&routed_b, &handle_a),
        "app B must NOT route to app A's handle"
    );

    nmp_ffi::nmp_app_free(app_a_ptr);
    nmp_ffi::nmp_app_free(app_b_ptr);
}
