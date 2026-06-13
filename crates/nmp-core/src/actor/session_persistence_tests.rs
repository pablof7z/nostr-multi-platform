use super::commands::{self, IdentityRuntime};
use super::session_persistence::{
    enqueue_persist_active_pointer, enqueue_persist_current_active_session,
    enqueue_persist_remote_signer_payload, restore_active_session,
};
use crate::actor::capability_worker::spawn_capability_worker;
use crate::actor::{ActorCommand, ActorMail, CommandSender};
use crate::bunker_hook::BunkerHookRequest;
use crate::capability_socket::{CapabilityCallbackRegistration, CapabilityCallbackSlot};
use crate::external_signer_hook::ExternalSignerHookRequest;
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::substrate::{CapabilityEnvelope, KeyringRequest, KeyringResult};
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

static STORE: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
static SERIAL: Mutex<()> = Mutex::new(());

extern "C" fn mock_handler(_ctx: *mut c_void, request_json: *const c_char) -> *mut c_char {
    let request = unsafe { CStr::from_ptr(request_json) }
        .to_str()
        .unwrap_or("");
    let parsed: serde_json::Value = serde_json::from_str(request).unwrap_or_default();
    let correlation_id = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let payload = parsed
        .get("payload_json")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let result = match serde_json::from_str::<KeyringRequest>(payload) {
        Ok(KeyringRequest::Store { account_id, secret }) => {
            STORE
                .lock()
                .unwrap()
                .get_or_insert_with(HashMap::new)
                .insert(account_id, secret);
            KeyringResult::ok(None)
        }
        Ok(KeyringRequest::Retrieve { account_id }) => {
            match STORE
                .lock()
                .unwrap()
                .get_or_insert_with(HashMap::new)
                .get(&account_id)
            {
                Some(secret) => KeyringResult::ok(Some(secret.clone())),
                None => KeyringResult::not_found(),
            }
        }
        Ok(KeyringRequest::Delete { account_id }) => {
            STORE
                .lock()
                .unwrap()
                .get_or_insert_with(HashMap::new)
                .remove(&account_id);
            KeyringResult::ok(None)
        }
        Err(_) => KeyringResult::error(-50),
    };

    let envelope = CapabilityEnvelope {
        namespace: "nmp.keyring.capability".to_string(),
        correlation_id,
        result_json: serde_json::to_string(&result).unwrap(),
    };
    CString::new(serde_json::to_string(&envelope).unwrap())
        .unwrap()
        .into_raw()
}

fn registered_slot() -> CapabilityCallbackSlot {
    let slot = crate::capability_socket::new_capability_callback_slot();
    *slot.lock().unwrap() = Some(CapabilityCallbackRegistration {
        context: 0,
        callback: mock_handler,
    });
    slot
}

fn fresh() -> (IdentityRuntime, Kernel) {
    (
        IdentityRuntime::new(
            commands::new_bunker_handshake_slot(),
            commands::new_signer_state_slot(),
        ),
        Kernel::new(DEFAULT_VISIBLE_LIMIT),
    )
}

/// Helper: spawn a capability worker and drain exactly `count` results.
///
/// The enqueue functions are async (fire-and-forget); in tests we need
/// the writes to complete before the synchronous restore reads. This
/// helper blocks until `count` `CapabilityResultReady` commands arrive
/// on the actor command channel, confirming every enqueued write has
/// been executed by the worker.
fn drain_worker_results(
    cmd_rx: &Receiver<ActorMail>,
    count: usize,
) {
    for _ in 0..count {
        cmd_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("CapabilityResultReady not received in time");
    }
}

#[test]
fn restores_imported_nsec_without_swift_cache() {
    let _g = SERIAL.lock().unwrap();
    *STORE.lock().unwrap() = Some(HashMap::new());
    let slot = registered_slot();
    let (inbox_tx, cmd_rx): (Sender<ActorMail>, Receiver<ActorMail>) = channel();
    let work_tx = spawn_capability_worker(Arc::clone(&slot), CommandSender::new(inbox_tx));

    let (mut identity, mut kernel) = fresh();
    commands::add_signer(
        &mut identity,
        &mut kernel,
        crate::actor::SignerSource::LocalNsec(zeroize::Zeroizing::new(TEST_NSEC.to_string())),
        true,
        false,
    );
    let expected = identity.active_pubkey().unwrap();

    // persist_current_active_session (local account) enqueues 3 writes:
    // local_nsec, active_id, active_kind.
    enqueue_persist_current_active_session(&identity, &work_tx);
    drain_worker_results(&cmd_rx, 3);

    let (mut restored_identity, mut restored_kernel) = fresh();
    // restore_active_session is synchronous (cold-start read chain).
    let (restore_work_tx, _restore_cmd_rx) = {
        let (tx, rx) = channel::<ActorMail>();
        let wtx = spawn_capability_worker(Arc::clone(&slot), CommandSender::new(tx));
        (wtx, rx)
    };
    restore_active_session(
        &mut restored_identity,
        &mut restored_kernel,
        &slot,
        &restore_work_tx,
        false,
    );

    assert_eq!(restored_identity.active_pubkey(), Some(expected.clone()));
    let (accounts, active) = restored_kernel.account_snapshot();
    assert_eq!(accounts.len(), 1);
    assert_eq!(active, Some(&expected));
}

#[test]
fn persists_generated_account_for_next_launch() {
    let _g = SERIAL.lock().unwrap();
    *STORE.lock().unwrap() = Some(HashMap::new());
    let slot = registered_slot();
    let (inbox_tx, cmd_rx): (Sender<ActorMail>, Receiver<ActorMail>) = channel();
    let work_tx = spawn_capability_worker(Arc::clone(&slot), CommandSender::new(inbox_tx));

    let (mut identity, mut kernel) = fresh();
    commands::create_account(
        &mut identity,
        &mut kernel,
        false,
        &HashMap::new(),
        &[],
        false,
        true,
    );
    let expected = identity.active_pubkey().unwrap();

    // persist_current_active_session (local account) enqueues 3 writes.
    enqueue_persist_current_active_session(&identity, &work_tx);
    drain_worker_results(&cmd_rx, 3);

    let (mut restored_identity, mut restored_kernel) = fresh();
    let (restore_work_tx, _restore_cmd_rx) = {
        let (tx, rx) = channel::<ActorMail>();
        let wtx = spawn_capability_worker(Arc::clone(&slot), CommandSender::new(tx));
        (wtx, rx)
    };
    restore_active_session(
        &mut restored_identity,
        &mut restored_kernel,
        &slot,
        &restore_work_tx,
        false,
    );

    assert_eq!(restored_identity.active_pubkey(), Some(expected.clone()));
    assert_eq!(restored_kernel.account_snapshot().1, Some(&expected));
}

#[test]
fn restores_nip46_from_persisted_remote_payload() {
    let _g = SERIAL.lock().unwrap();
    *STORE.lock().unwrap() = Some(HashMap::new());
    let slot = registered_slot();
    let (inbox_tx, cmd_rx): (Sender<ActorMail>, Receiver<ActorMail>) = channel();
    let work_tx = spawn_capability_worker(Arc::clone(&slot), CommandSender::new(inbox_tx));

    let identity_id = "701eb015134aed0cb6582a86b9527f2db0241ca36a64bfd63ddbde59002c7c05";
    let payload_json = format!(
        r#"{{"kind":"nip46","body":{{"local_secret_hex":"{}","remote_pubkey_hex":"{}","relays":["wss://relay.example"],"secret":"testsecret","permissions":null,"cached_remote_user_pubkey_hex":"{}"}}}}"#,
        "00".repeat(32),
        identity_id,
        identity_id
    );

    // Enqueue remote-payload persist (1 write) and active-pointer (2 writes).
    enqueue_persist_remote_signer_payload(identity_id, &payload_json, &work_tx);
    enqueue_persist_active_pointer(&work_tx, identity_id, "nip46");
    drain_worker_results(&cmd_rx, 3);

    let calls: Arc<Mutex<Vec<BunkerHookRequest>>> = Arc::new(Mutex::new(Vec::new()));
    let calls_clone = Arc::clone(&calls);

    let (mut identity, mut kernel) = fresh();
    // ADR-0052 §D3 — install the recording hook into THIS runtime's per-app
    // slot (no process-global). Restore reads it at invocation time, so
    // install order relative to `fresh()` is irrelevant beyond pre-restore.
    identity.install_bunker_hook_for_test(Arc::new(move |request| {
        calls_clone.lock().unwrap().push(request);
    }));
    let (restore_work_tx, _restore_cmd_rx) = {
        let (tx, rx) = channel::<ActorMail>();
        let wtx = spawn_capability_worker(Arc::clone(&slot), CommandSender::new(tx));
        (wtx, rx)
    };
    let _outbound = restore_active_session(
        &mut identity,
        &mut kernel,
        &slot,
        &restore_work_tx,
        false,
    );

    assert_eq!(
        calls.lock().unwrap().as_slice(),
        &[BunkerHookRequest::Restore { payload_json }]
    );
    // D0: handshake state is an app noun — `restore_bunker_session` seeds it
    // into the identity runtime's shared slot (read by the
    // `"bunker_handshake"` projection), not a typed kernel field.
    let progress = identity.bunker_handshake_for_test().expect("progress");
    assert_eq!(progress.stage, "connecting");
}

/// Cold-start regression for issue #1238: a persisted NIP-55 remote-signer
/// payload, plus a registered external-signer hook, drives the actor's
/// cold-start `restore_active_session` to hand the opaque payload to the
/// hook exactly once via `ExternalSignerHookRequest::Restore`. This is the
/// symmetric counterpart of `restores_nip46_from_persisted_remote_payload`
/// (NIP-46 routes through the bunker hook; NIP-55 through the external-signer
/// hook). ADR-0052 §D3 — the hook is a per-app slot read at restore invocation
/// time, so the suspected init-order bug (restore before install) does not
/// exist: install-before-restore is the only ordering this test relies on.
#[test]
fn restores_nip55_from_persisted_remote_payload() {
    let _g = SERIAL.lock().unwrap();
    *STORE.lock().unwrap() = Some(HashMap::new());
    let slot = registered_slot();
    let (inbox_tx, cmd_rx): (Sender<ActorMail>, Receiver<ActorMail>) = channel();
    let work_tx = spawn_capability_worker(Arc::clone(&slot), CommandSender::new(inbox_tx));

    let identity_id = "701eb015134aed0cb6582a86b9527f2db0241ca36a64bfd63ddbde59002c7c05";
    // Opaque-to-nmp-core NIP-55 payload (ADR-0048 D4 — pubkey-only): the
    // kernel persists and replays it verbatim, never parsing the body.
    let payload_json = format!(
        r#"{{"kind":"nip55","body":{{"user_pubkey_hex":"{}","signer_package":"com.greenart7c3.nostrsigner","granted_permissions":["sign_event","get_public_key"]}}}}"#,
        identity_id
    );

    // Enqueue remote-payload persist (1 write) and active-pointer (2 writes).
    enqueue_persist_remote_signer_payload(identity_id, &payload_json, &work_tx);
    enqueue_persist_active_pointer(&work_tx, identity_id, "nip55");
    drain_worker_results(&cmd_rx, 3);

    let calls: Arc<Mutex<Vec<ExternalSignerHookRequest>>> = Arc::new(Mutex::new(Vec::new()));
    let calls_clone = Arc::clone(&calls);

    let (mut identity, mut kernel) = fresh();
    // ADR-0052 §D3 — install the recording hook into THIS runtime's per-app
    // slot (no process-global).
    identity.install_external_signer_hook_for_test(Arc::new(move |request| {
        calls_clone.lock().unwrap().push(request);
    }));
    let (restore_work_tx, _restore_cmd_rx) = {
        let (tx, rx) = channel::<ActorMail>();
        let wtx = spawn_capability_worker(Arc::clone(&slot), CommandSender::new(tx));
        (wtx, rx)
    };
    let _outbound = restore_active_session(
        &mut identity,
        &mut kernel,
        &slot,
        &restore_work_tx,
        false,
    );

    assert_eq!(
        calls.lock().unwrap().as_slice(),
        &[ExternalSignerHookRequest::Restore { payload_json }]
    );
}

/// D6 negative companion to `restores_nip55_from_persisted_remote_payload`:
/// when NO external-signer hook is registered, cold-start NIP-55 restore must
/// NOT silently no-op. It surfaces a degraded `signer_state` ("unavailable")
/// and a `last_error_toast` so the host can prompt the user — the D6
/// observable-degradation guarantee, never silence.
#[test]
fn restore_nip55_without_hook_surfaces_unavailable_and_toast() {
    let _g = SERIAL.lock().unwrap();
    *STORE.lock().unwrap() = Some(HashMap::new());
    let slot = registered_slot();
    let (inbox_tx, cmd_rx): (Sender<ActorMail>, Receiver<ActorMail>) = channel();
    let work_tx = spawn_capability_worker(Arc::clone(&slot), CommandSender::new(inbox_tx));

    let identity_id = "701eb015134aed0cb6582a86b9527f2db0241ca36a64bfd63ddbde59002c7c05";
    let payload_json = format!(
        r#"{{"kind":"nip55","body":{{"user_pubkey_hex":"{}","signer_package":"com.greenart7c3.nostrsigner","granted_permissions":["sign_event"]}}}}"#,
        identity_id
    );

    enqueue_persist_remote_signer_payload(identity_id, &payload_json, &work_tx);
    enqueue_persist_active_pointer(&work_tx, identity_id, "nip55");
    drain_worker_results(&cmd_rx, 3);

    // ADR-0052 §D3 — the external-signer hook is now a PER-APP slot. A fresh
    // `IdentityRuntime` starts with an EMPTY slot, so this test exercises the
    // *no hook* (D6 degradation) branch deterministically — no process-global
    // to clear, no execution-order dependence (the old global needed a
    // test-only `clear_external_signer_hook_for_test` workaround).
    let (mut identity, mut kernel) = fresh();
    let (restore_work_tx, _restore_cmd_rx) = {
        let (tx, rx) = channel::<ActorMail>();
        let wtx = spawn_capability_worker(Arc::clone(&slot), CommandSender::new(tx));
        (wtx, rx)
    };
    let _outbound = restore_active_session(
        &mut identity,
        &mut kernel,
        &slot,
        &restore_work_tx,
        false,
    );

    let signer_state = identity
        .signer_state_for_test()
        .expect("signer_state must be set on degraded NIP-55 restore");
    assert_eq!(signer_state.signer_kind, "nip55");
    assert_eq!(signer_state.state, "unavailable");
    assert!(signer_state.is_unavailable);
    assert!(
        kernel.last_error_toast_snapshot().is_some(),
        "D6: missing NIP-55 driver must surface a last_error_toast, not silence"
    );
}
