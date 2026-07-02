//! K2 rung 5.3 oracle — per-app bunker + NIP-55 signer ports.
//!
//! ADR-0072 §D3. Proves the four process-globals that K2 rung 5.3 deletes
//! (`GLOBAL_BROKER`, `GLOBAL_DRIVER`, and the two `nmp-core` hook statics
//! `bunker_hook::HOOK` / `external_signer_hook::HOOK`) are replaced by per-app
//! `Arc` slots created in `nmp_app_new` and dropped in `nmp_app_free` — the
//! same "no global aliasing across `nmp_app_free`" invariant the rest of
//! `NmpApp`'s slots obey (ADR-0072 / ADR-0072 relay-connected hook slot).
//!
//! ## Why this DEAD-ENDS against the current global design
//!
//! Today the bunker hook lives in a `static HOOK: OnceLock<…>` and the broker
//! in a `static GLOBAL_BROKER: OnceLock<Arc<BunkerBroker>>`. A `OnceLock` fires
//! exactly once per process:
//!
//! * **Free-then-recreate** (the Android process-reuse failure mode): after
//!   `nmp_app_free`, a freshly `nmp_app_new`'d app cannot reinstall the hook —
//!   the `OnceLock` is already fired, so `get_or_init` keeps the *dead* app's
//!   broker/hook and the new app dead-ends. This test asserts the recreated
//!   app's freshly-installed hook is the one that fires.
//! * **Two-instance crosstalk**: two `NmpApp`s share one global hook, so an
//!   invocation on app B would run app A's hook. This test asserts each app's
//!   invocation routes to *its own* recording hook only.
//!
//! After rung 5.3 (per-app slots) both assertions pass.
//!
//! ## Routing / correlation
//!
//! There is intentionally NO correlation token on `BunkerHookRequest`: routing
//! a broker response back to the originating app is achieved structurally by
//! the per-app `CommandSender` the broker captures (each app's broker sends
//! `AddSigner`/`DeliverSignerResponse` to its OWN actor inbox). The per-app
//! hook slot proven here is the other half of that same per-app wiring.

use std::sync::{Arc, Mutex};

use nmp_core::{BunkerHookRequest, ExternalSignerHookRequest};
use nmp_native_runtime::{
    install_bunker_hook_for_test, install_external_signer_hook_for_test,
    invoke_bunker_connect_hook_for_test, invoke_external_signer_restore_hook_for_test,
};

/// Two concurrent apps each route their bunker-hook invocation to their OWN
/// installed hook — zero crosstalk. Fails on the global design: one shared
/// `OnceLock` hook means app B's invocation would land in app A's log.
#[test]
fn two_apps_route_bunker_hook_to_their_own_slot() {
    let app_a = Box::into_raw(Box::new(nmp_native_runtime::new_app()));
    let app_b = Box::into_raw(Box::new(nmp_native_runtime::new_app()));

    let log_a: Arc<Mutex<Vec<BunkerHookRequest>>> = Arc::new(Mutex::new(Vec::new()));
    let log_b: Arc<Mutex<Vec<BunkerHookRequest>>> = Arc::new(Mutex::new(Vec::new()));

    {
        let log = Arc::clone(&log_a);
        install_bunker_hook_for_test(app_a, Arc::new(move |req| log.lock().unwrap().push(req)));
    }
    {
        let log = Arc::clone(&log_b);
        install_bunker_hook_for_test(app_b, Arc::new(move |req| log.lock().unwrap().push(req)));
    }

    // Invoke each app's per-app slot directly (the test-support seam routes
    // through the SAME slot the actor's `start_bunker_handshake` reads).
    assert!(invoke_bunker_connect_hook_for_test(app_a, "bunker://app-a"));
    assert!(invoke_bunker_connect_hook_for_test(app_b, "bunker://app-b"));

    let seen_a = log_a.lock().unwrap().clone();
    let seen_b = log_b.lock().unwrap().clone();

    unsafe { drop(Box::from_raw(app_a)) };
    unsafe { drop(Box::from_raw(app_b)) };

    assert_eq!(
        seen_a.as_slice(),
        &[BunkerHookRequest::Connect {
            uri: "bunker://app-a".to_string()
        }],
        "app A's slot must route only app A's invocation"
    );
    assert_eq!(
        seen_b.as_slice(),
        &[BunkerHookRequest::Connect {
            uri: "bunker://app-b".to_string()
        }],
        "app B's slot must route only app B's invocation — no crosstalk"
    );
}

/// Free an app, then create a NEW app and install a fresh bunker hook: the new
/// hook fires. This is the Android process-reuse dead-end — a `OnceLock`-backed
/// global stays fired across `nmp_app_free`, so the recreated app's new hook
/// would never be installed and the invocation would run the dead app's hook
/// (or return `false`). Per-app slots re-initialise cleanly.
#[test]
fn freed_then_recreated_app_reinstalls_bunker_hook() {
    // (1) First app installs a hook and proves it fires.
    let first = Box::into_raw(Box::new(nmp_native_runtime::new_app()));
    let first_log: Arc<Mutex<Vec<BunkerHookRequest>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let log = Arc::clone(&first_log);
        install_bunker_hook_for_test(first, Arc::new(move |req| log.lock().unwrap().push(req)));
    }
    assert!(invoke_bunker_connect_hook_for_test(first, "bunker://first"));
    assert_eq!(first_log.lock().unwrap().len(), 1);

    // (2) Free it (drops the per-app slot).
    unsafe { drop(Box::from_raw(first)) };

    // (3) A brand-new app installs its OWN hook and it must fire — this is the
    //     assertion the `OnceLock` global cannot satisfy.
    let second = Box::into_raw(Box::new(nmp_native_runtime::new_app()));
    let second_log: Arc<Mutex<Vec<BunkerHookRequest>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let log = Arc::clone(&second_log);
        install_bunker_hook_for_test(second, Arc::new(move |req| log.lock().unwrap().push(req)));
    }
    let fired = invoke_bunker_connect_hook_for_test(second, "bunker://second");
    let seen = second_log.lock().unwrap().clone();
    unsafe { drop(Box::from_raw(second)) };

    assert!(
        fired,
        "recreated app must have a freshly-installed bunker hook (Android process-reuse)"
    );
    assert_eq!(
        seen.as_slice(),
        &[BunkerHookRequest::Connect {
            uri: "bunker://second".to_string()
        }]
    );
    // The dead app's log saw exactly its one historical call — no leakage.
    assert_eq!(first_log.lock().unwrap().len(), 1);
}

/// The NIP-55 external-signer hook gets the same per-app treatment (structural
/// twin of the bunker hook). Two apps, two restore hooks, zero crosstalk +
/// recreate works.
#[test]
fn external_signer_hook_is_per_app_and_survives_recreate() {
    let app_a = Box::into_raw(Box::new(nmp_native_runtime::new_app()));
    let log_a: Arc<Mutex<Vec<ExternalSignerHookRequest>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let log = Arc::clone(&log_a);
        install_external_signer_hook_for_test(
            app_a,
            Arc::new(move |req| log.lock().unwrap().push(req)),
        );
    }
    let fired_a = invoke_external_signer_restore_hook_for_test(app_a, "payload-a");
    let seen_a = log_a.lock().unwrap().clone();
    unsafe { drop(Box::from_raw(app_a)) };

    assert!(fired_a);
    assert_eq!(
        seen_a.as_slice(),
        &[ExternalSignerHookRequest::Restore {
            payload_json: "payload-a".to_string()
        }]
    );

    // Recreated app installs a fresh hook — fires cleanly (no fired-once global).
    let app_c = Box::into_raw(Box::new(nmp_native_runtime::new_app()));
    let log_c: Arc<Mutex<Vec<ExternalSignerHookRequest>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let log = Arc::clone(&log_c);
        install_external_signer_hook_for_test(
            app_c,
            Arc::new(move |req| log.lock().unwrap().push(req)),
        );
    }
    let fired_c = invoke_external_signer_restore_hook_for_test(app_c, "payload-c");
    let seen_c = log_c.lock().unwrap().clone();
    unsafe { drop(Box::from_raw(app_c)) };

    assert!(fired_c);
    assert_eq!(
        seen_c.as_slice(),
        &[ExternalSignerHookRequest::Restore {
            payload_json: "payload-c".to_string()
        }]
    );
}
