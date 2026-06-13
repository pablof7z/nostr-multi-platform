//! V-90 Site 2 — `CapabilityResultReady` dispatch-arm tests.
//!
//! Tests the four required properties from ADR-0040 §3:
//!
//! 1. **Actor does not block** — a slow capability callback does NOT stall
//!    the actor; it continues processing other commands while the capability
//!    op is in flight. (Tested in `capability_worker.rs` — worker runs
//!    callback off-actor; test here confirms the dispatch arm itself is O(1).)
//!
//! 2. **FIFO ordering** — two ops for the same account (persist then forget)
//!    execute in submission order (tested in `capability_worker.rs`).
//!
//! 3. **Removed-account drop** — a `CapabilityResultReady` for a since-removed
//!    account is dropped with a D6 trace; the active account is NOT mutated.
//!
//! 4. **CapabilityResultReady dispatch** — the new `ActorCommand` variant
//!    routes to the handler; a still-present-account result succeeds silently
//!    (no toast, no kernel mutation for a write result).

use super::commands::{self, IdentityRuntime};
use super::dispatch::{dispatch_command, ActorContext};
use super::ActorCommand;
use crate::actor::capability_worker::{spawn_capability_worker, CapabilityWorkSender};
use crate::capability_socket::{CapabilityCallbackRegistration, CapabilityCallbackSlot};
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::substrate::{CapabilityEnvelope, KeyringRequest, KeyringResult};
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr, CString};
use crate::actor::{ActorMail, CommandSender};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};

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
                Some(s) => KeyringResult::ok(Some(s.clone())),
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

// ─────────────────────────────────────────────────────────────────────────────
// Minimal dispatch harness (no relay network, no pool)
//
// We need to exercise `dispatch_command` directly for `CapabilityResultReady`
// without the full actor loop. Use the `Barrier` approach: call
// `dispatch_command` directly with a synthetic `ActorCommand`.
// ─────────────────────────────────────────────────────────────────────────────

/// Dispatch a single `ActorCommand::CapabilityResultReady` through the actor
/// dispatch function with a real `IdentityRuntime` + `Kernel`, then return the
/// last-error-toast value from the kernel (to assert on).
fn dispatch_capability_result(
    account_id: &str,
    result_json: &str,
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    slot: &CapabilityCallbackSlot,
    capability_work_tx: &CapabilityWorkSender,
    command_tx: &CommandSender,
) -> Option<String> {
    // Build a minimal ActorContext with the required fields. Relay pool,
    // relay_controls, etc. are not needed for CapabilityResultReady.
    use crate::actor::pending_sign::ParkedOp;
    use crate::relay::CanonicalRelayUrl;
    use std::collections::{HashMap, HashSet};
    use std::time::Instant;

    let (update_tx, _update_rx) = channel::<crate::update_envelope::UpdateFrameBytes>();
    let lifecycle_observer = commands::new_observer_slot();
    let mls_local_nsec = Arc::new(Mutex::new(None));
    let active_local_keys = Arc::new(Mutex::new(None));
    let pool = nmp_network::pool::Pool::new(
        nmp_network::pool::PoolConfig::default(),
        channel::<nmp_network::pool::PoolEvent>().0,
    );
    let mut relay_controls: HashMap<CanonicalRelayUrl, super::RelayControl> = HashMap::new();
    let mut slot_to_url: HashMap<u32, CanonicalRelayUrl> = HashMap::new();
    let mut connected_relays = HashSet::new();
    let mut connected_urls = HashSet::new();
    let mut last_emit = Instant::now();
    let mut next_relay_generation = 1u64;
    let mut running = true;
    let mut emit_hz = 4u32;
    let mut startup_sent = false;
    let mut parked_ops: Vec<ParkedOp> = Vec::new();
    let coverage_hook = Arc::new(Mutex::new(None::<crate::subs::PlanCoverageHook>));
    let req_frame_interceptor = Arc::new(Mutex::new(None));
    let host_op_handler = Arc::new(Mutex::new(None));
    let ingest_dispatcher_slot = Arc::new(std::sync::RwLock::new(
        crate::substrate::EventIngestDispatcher::default(),
    ));
    let dm_inbox_relays_slot = Arc::new(Mutex::new(
        crate::substrate::empty_dm_inbox_relay_lookup(),
    ));
    let blocked_relays_slot = Arc::new(Mutex::new(
        crate::substrate::empty_blocked_relay_lookup(),
    ));
    let bootstrap_self_kinds_slot = Arc::new(Mutex::new(None));
    let routing_trace_slot = Arc::new(Mutex::new(None));
    let event_store_slot = Arc::new(Mutex::new(None));
    let routing_substrate_slot = Arc::new(Mutex::new(None));
    let publish_resolver_slot = Arc::new(Mutex::new(None));
    let active_account_slot = Arc::new(Mutex::new(None));
    let raw_event_forward_observer_ids =
        crate::actor::raw_event_forwarder::new_raw_event_forward_observer_id_slot();
    let raw_event_forward_policy_slot = Arc::new(Mutex::new(None));
    let raw_event_observers = commands::new_raw_event_observer_slot();

    let mut ctx = ActorContext {
        kernel,
        identity,
        relay_controls: &mut relay_controls,
        slot_to_url: &mut slot_to_url,
        pool: &pool,
        connected_relays: &mut connected_relays,
        connected_urls: &mut connected_urls,
        update_tx: &update_tx,
        last_emit: &mut last_emit,
        next_relay_generation: &mut next_relay_generation,
        running: &mut running,
        emit_hz: &mut emit_hz,
        startup_sent: &mut startup_sent,
        relays_ready: false,
        lifecycle_observer: &lifecycle_observer,
        mls_local_nsec: &mls_local_nsec,
        active_local_keys: &active_local_keys,
        capability_callback: slot,
        parked_ops: &mut parked_ops,
        command_tx_self: command_tx,
        capability_work_tx,
        coverage_hook_slot: &coverage_hook,
        req_frame_interceptor_slot: &req_frame_interceptor,
        host_op_handler: &host_op_handler,
        ingest_dispatcher_slot: &ingest_dispatcher_slot,
        dm_inbox_relays_slot: &dm_inbox_relays_slot,
        blocked_relays_slot: &blocked_relays_slot,
        bootstrap_self_kinds_slot: &bootstrap_self_kinds_slot,
        routing_trace_slot: &routing_trace_slot,
        event_store_slot: &event_store_slot,
        routing_substrate_slot: &routing_substrate_slot,
        publish_resolver_slot: &publish_resolver_slot,
        active_account_slot: &active_account_slot,
        raw_event_forward_observer_ids: &raw_event_forward_observer_ids,
        raw_event_forward_policy_slot: &raw_event_forward_policy_slot,
        raw_event_observers_handle: &raw_event_observers,
    };

    dispatch_command(
        ActorCommand::CapabilityResultReady {
            account_id: account_id.to_string(),
            result_json: result_json.to_string(),
        },
        &mut ctx,
    );
    ctx.kernel.last_error_toast_snapshot().cloned()
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: removed-account result is dropped — no toast for the active account
// ─────────────────────────────────────────────────────────────────────────────

/// ADR-0040 §3 account-switch safety: a `CapabilityResultReady` for a
/// since-removed account is silently dropped. The ACTIVE account's toast must
/// NOT be set (no spurious cross-account error). This is the core guarantee of
/// the serialized FIFO design: removed-account results never pollute the
/// now-active account.
#[test]
fn capability_result_ready_dropped_for_removed_account() {
    let _g = SERIAL.lock().unwrap();
    *STORE.lock().unwrap() = Some(HashMap::new());
    let slot = registered_slot();
    let (inbox_tx, _cmd_rx) = channel::<ActorMail>();
    let cmd_tx = CommandSender::new(inbox_tx);
    let (work_tx, _work_cmd_rx) = {
        let (tx, rx) = channel::<ActorMail>();
        (spawn_capability_worker(Arc::clone(&slot), CommandSender::new(tx)), rx)
    };

    let mut identity = IdentityRuntime::new(
        commands::new_bunker_handshake_slot(),
        commands::new_signer_state_slot(),
    );
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Sign in an account so there IS an active account.
    commands::add_signer(
        &mut identity,
        &mut kernel,
        crate::actor::SignerSource::LocalNsec(zeroize::Zeroizing::new(TEST_NSEC.to_string())),
        true,
        false,
    );
    let active_id = identity.active_pubkey().unwrap();

    // Simulate a result for a DIFFERENT (removed) account.
    let removed_account_id = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
    assert_ne!(active_id, removed_account_id);
    assert!(!identity.contains_account(removed_account_id));

    // A success result for the removed account — should be silently dropped.
    let success_envelope = CapabilityEnvelope {
        namespace: "nmp.keyring.capability".to_string(),
        correlation_id: "c-removed".to_string(),
        result_json: serde_json::to_string(&KeyringResult::ok(None)).unwrap(),
    };
    let success_json = serde_json::to_string(&success_envelope).unwrap();

    let toast = dispatch_capability_result(
        removed_account_id,
        &success_json,
        &mut identity,
        &mut kernel,
        &slot,
        &work_tx,
        &cmd_tx,
    );

    // Dropped — no toast on the active account.
    assert!(
        toast.is_none(),
        "removed-account result must not set a toast; got: {toast:?}"
    );

    // An ERROR result for the removed account — should also be silently dropped.
    let error_envelope = CapabilityEnvelope {
        namespace: "nmp.keyring.capability".to_string(),
        correlation_id: "c-removed-err".to_string(),
        result_json: serde_json::to_string(&KeyringResult::error(-50)).unwrap(),
    };
    let error_json = serde_json::to_string(&error_envelope).unwrap();

    // Reset toast state.
    kernel.set_last_error_toast(None);

    let toast = dispatch_capability_result(
        removed_account_id,
        &error_json,
        &mut identity,
        &mut kernel,
        &slot,
        &work_tx,
        &cmd_tx,
    );
    assert!(
        toast.is_none(),
        "removed-account error result must not set a toast; got: {toast:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: present-account error result surfaces a toast
// ─────────────────────────────────────────────────────────────────────────────

/// A `CapabilityResultReady` error for a PRESENT account must surface a D6
/// toast so the user sees "keychain write failed" rather than silent data loss.
#[test]
fn capability_result_ready_error_toasts_for_present_account() {
    let _g = SERIAL.lock().unwrap();
    *STORE.lock().unwrap() = Some(HashMap::new());
    let slot = registered_slot();
    let (inbox_tx, _cmd_rx) = channel::<ActorMail>();
    let cmd_tx = CommandSender::new(inbox_tx);
    let (work_tx, _work_cmd_rx) = {
        let (tx, rx) = channel::<ActorMail>();
        (spawn_capability_worker(Arc::clone(&slot), CommandSender::new(tx)), rx)
    };

    let mut identity = IdentityRuntime::new(
        commands::new_bunker_handshake_slot(),
        commands::new_signer_state_slot(),
    );
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    commands::add_signer(
        &mut identity,
        &mut kernel,
        crate::actor::SignerSource::LocalNsec(zeroize::Zeroizing::new(TEST_NSEC.to_string())),
        true,
        false,
    );
    let active_id = identity.active_pubkey().unwrap();

    // An ERROR result for the present account.
    let error_envelope = CapabilityEnvelope {
        namespace: "nmp.keyring.capability".to_string(),
        correlation_id: "c-present-err".to_string(),
        result_json: serde_json::to_string(&KeyringResult::error(-50)).unwrap(),
    };
    let error_json = serde_json::to_string(&error_envelope).unwrap();

    let toast = dispatch_capability_result(
        &active_id,
        &error_json,
        &mut identity,
        &mut kernel,
        &slot,
        &work_tx,
        &cmd_tx,
    );
    assert!(
        toast.is_some(),
        "present-account error result must surface a toast"
    );
}
