use super::commands::{self, IdentityRuntime};
use super::session_persistence::{
    enqueue_persist_active_pointer, enqueue_persist_current_active_session,
    enqueue_persist_remote_signer_payload, restore_active_session,
};
use crate::actor::capability_worker::spawn_capability_worker;
use crate::actor::ActorCommand;
use crate::bunker_hook::BunkerHookRequest;
use crate::capability_socket::{CapabilityCallbackRegistration, CapabilityCallbackSlot};
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
            commands::new_bunker_connection_state_slot(),
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
    cmd_rx: &Receiver<ActorCommand>,
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
    let (cmd_tx, cmd_rx): (Sender<ActorCommand>, Receiver<ActorCommand>) = channel();
    let work_tx = spawn_capability_worker(Arc::clone(&slot), cmd_tx);

    let (mut identity, mut kernel) = fresh();
    commands::sign_in_nsec(&mut identity, &mut kernel, TEST_NSEC, false);
    let expected = identity.active_pubkey().unwrap();

    // persist_current_active_session (local account) enqueues 3 writes:
    // local_nsec, active_id, active_kind.
    enqueue_persist_current_active_session(&identity, &work_tx);
    drain_worker_results(&cmd_rx, 3);

    let (mut restored_identity, mut restored_kernel) = fresh();
    // restore_active_session is synchronous (cold-start read chain).
    let (restore_work_tx, _restore_cmd_rx) = {
        let (tx, rx) = channel::<ActorCommand>();
        let wtx = spawn_capability_worker(Arc::clone(&slot), tx);
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
    let (cmd_tx, cmd_rx): (Sender<ActorCommand>, Receiver<ActorCommand>) = channel();
    let work_tx = spawn_capability_worker(Arc::clone(&slot), cmd_tx);

    let (mut identity, mut kernel) = fresh();
    commands::create_account(
        &mut identity,
        &mut kernel,
        false,
        &HashMap::new(),
        &[],
        false,
    );
    let expected = identity.active_pubkey().unwrap();

    // persist_current_active_session (local account) enqueues 3 writes.
    enqueue_persist_current_active_session(&identity, &work_tx);
    drain_worker_results(&cmd_rx, 3);

    let (mut restored_identity, mut restored_kernel) = fresh();
    let (restore_work_tx, _restore_cmd_rx) = {
        let (tx, rx) = channel::<ActorCommand>();
        let wtx = spawn_capability_worker(Arc::clone(&slot), tx);
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
    let (cmd_tx, cmd_rx): (Sender<ActorCommand>, Receiver<ActorCommand>) = channel();
    let work_tx = spawn_capability_worker(Arc::clone(&slot), cmd_tx);

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
    crate::bunker_hook::register_bunker_hook(Arc::new(move |request| {
        calls_clone.lock().unwrap().push(request);
    }));

    let (mut identity, mut kernel) = fresh();
    let (restore_work_tx, _restore_cmd_rx) = {
        let (tx, rx) = channel::<ActorCommand>();
        let wtx = spawn_capability_worker(Arc::clone(&slot), tx);
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
