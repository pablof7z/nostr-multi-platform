//! Unit tests for the NIP-55 external-signer driver — extracted from
//! `external_signer.rs` to keep that file under the 500-LOC ceiling
//! (same pattern as the actor's sibling `*_tests.rs` modules).

//! Driver loop tests — the Rust half of the ADR-0048 Stage-2 done-gate:
//! signin builds the `get_public_key` request (with the permission
//! batch), the host's raw reply resolves into `AddSigner { RemoteHandle
//! (nip55), make_active }` + `signer_state: ready`, and a subsequent
//! sign round-trips through the SAME transport and verifies end to end.

use super::*;
use nmp_core::actor::ActorCommand;
use nmp_core::actor::IdentityCommand;
use std::sync::mpsc;
use std::time::Duration;

use nmp_signer_iface::{ExternalSignerMethod, ExternalSignerOutcome};

/// Build a production driver wired through the REAL
/// `CapabilitySignerTransport` + capability socket, against a mock
/// native handler that acks every dispatch (the role the Android JNI
/// trampoline plays) and records the `payload_json` so tests can assert
/// on the exact request Rust built.
fn make_driver(tx: nmp_core::CommandSender) -> Nip55Driver {
    let slot = nmp_core::__ffi_internal::new_capability_callback_slot();
    install_dispatch_ack(&slot);
    let transport = Arc::new(CapabilitySignerTransport { callback: slot });
    Nip55Driver::new(tx, transport)
}

/// Test channel for the driver: a waking-inbox `CommandSender` (ADR-0050
/// §D3a) plus a receiver that unwraps `ActorMail::Command` so the
/// assertions below keep speaking `ActorCommand`.
fn cmd_channel() -> (nmp_core::CommandSender, CmdRx) {
    let (tx, rx) = mpsc::channel();
    (nmp_core::CommandSender::new(tx), CmdRx(rx))
}

struct CmdRx(mpsc::Receiver<nmp_core::ActorMail>);

impl CmdRx {
    fn try_recv(&self) -> Result<ActorCommand, mpsc::TryRecvError> {
        match self.0.try_recv()? {
            nmp_core::ActorMail::Command(cmd) => Ok(cmd),
            // The driver only ever sends commands; relay mail cannot
            // appear on this test channel.
            _ => Err(mpsc::TryRecvError::Empty),
        }
    }
}

/// Handler that acks every external_signer dispatch and records the
/// payload into a process-global so tests can assert on the request.
/// (FFI-shaped `extern "C" fn` cannot capture state — the keyring mock
/// in `capability.rs` uses the same pattern.)
static DISPATCHED: Mutex<Vec<String>> = Mutex::new(Vec::new());

extern "C" fn ack_handler(_ctx: *mut std::ffi::c_void, request_json: *const c_char) -> *mut c_char {
    let request = unsafe { std::ffi::CStr::from_ptr(request_json) }
        .to_string_lossy()
        .into_owned();
    let parsed: serde_json::Value = serde_json::from_str(&request).unwrap_or_default();
    let namespace = parsed
        .get("namespace")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let correlation_id = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if let Some(payload) = parsed.get("payload_json").and_then(|v| v.as_str()) {
        DISPATCHED.lock().unwrap().push(payload.to_string());
    }
    let envelope = CapabilityEnvelope {
        namespace,
        correlation_id,
        result_json: r#"{"status":"dispatched"}"#.to_string(),
    };
    std::ffi::CString::new(serde_json::to_string(&envelope).unwrap())
        .unwrap()
        .into_raw()
}

fn install_dispatch_ack(slot: &CapabilityCallbackSlot) {
    slot.set_registration(Some(
        nmp_core::__ffi_internal::CapabilityCallbackRegistration {
            context: 0,
            callback: ack_handler,
        },
    ));
}

fn last_dispatched_request() -> ExternalSignerRequest {
    let guard = DISPATCHED.lock().unwrap();
    serde_json::from_str(guard.last().expect("a request was dispatched"))
        .expect("dispatched payload parses as ExternalSignerRequest")
}

/// Serialise access to the DISPATCHED global across tests.
static SERIAL: Mutex<()> = Mutex::new(());

#[test]
fn signin_loop_resolves_add_signer_and_sign_round_trip() {
    let _g = SERIAL.lock().unwrap();
    DISPATCHED.lock().unwrap().clear();
    let (tx, rx) = cmd_channel();
    let driver = make_driver(tx);

    // 1. signin → awaiting_approval + a get_public_key dispatch with the
    //    permission batch (Rust decides what to ask for — D2).
    driver.signin(Some("com.greenart7c3.nostrsigner".to_string()));
    match rx.try_recv().expect("state command sent") {
        ActorCommand::Identity(IdentityCommand::Nip55SignerStateChanged { state, .. }) => {
            assert_eq!(state, "awaiting_approval");
        }
        other => panic!("expected Nip55SignerStateChanged, got {other:?}"),
    }
    let connect_request = last_dispatched_request();
    assert_eq!(connect_request.method, ExternalSignerMethod::GetPublicKey);
    assert!(
        !connect_request.permissions.is_empty(),
        "first connect must carry the permission batch"
    );
    assert_eq!(
        connect_request.signer_package.as_deref(),
        Some("com.greenart7c3.nostrsigner")
    );

    // 2. Host reports the raw pubkey reply → AddSigner{nip55, active}.
    let keys = nostr::Keys::generate();
    let reply = ExternalSignerResponse {
        correlation_id: connect_request.correlation_id.clone(),
        outcome: ExternalSignerOutcome::Ok {
            result: keys.public_key().to_hex(),
        },
        signer_package: Some("com.greenart7c3.nostrsigner".to_string()),
    };
    driver.deliver(&serde_json::to_string(&reply).unwrap());

    let handle = match rx.try_recv().expect("AddSigner sent") {
        ActorCommand::Identity(IdentityCommand::AddSigner {
            source: nmp_core::SignerSource::RemoteHandle(handle),
            make_active,
        }) => {
            assert!(make_active);
            handle
        }
        other => panic!("expected AddSigner, got {other:?}"),
    };
    assert_eq!(handle.signer_kind(), "nip55");
    assert_eq!(handle.pubkey_hex(), keys.public_key().to_hex());
    assert_eq!(handle.op_timeout(), Duration::from_secs(90));
    match rx.try_recv().expect("ready state sent") {
        ActorCommand::Identity(IdentityCommand::Nip55SignerStateChanged { state, .. }) => {
            assert_eq!(state, "ready")
        }
        other => panic!("expected ready state, got {other:?}"),
    }

    // 3. Sign through the actor-held handle → a sign_event dispatch on
    //    the SAME transport; the raw signed-event reply resolves the
    //    parked op with full id+sig verification (the mapper).
    let unsigned = UnsignedEvent {
        pubkey: keys.public_key().to_hex(),
        kind: 1,
        tags: vec![],
        content: "loop proof".to_string(),
        created_at: 1_700_000_000,
    };
    let op = handle.sign(&unsigned);

    let sign_request = last_dispatched_request();
    assert_eq!(sign_request.method, ExternalSignerMethod::SignEvent);
    assert!(
        sign_request.permissions.is_empty(),
        "non-connect ops must not re-send the permission batch"
    );

    // Stand in for Amber: actually sign the event the request asked for
    // (the mapper recomputes the id and verifies the schnorr signature,
    // so the reply must be a REAL signed event).
    let signed_json = amber_sign(&keys, &sign_request.payload);
    let sign_reply = ExternalSignerResponse {
        correlation_id: sign_request.correlation_id.clone(),
        outcome: ExternalSignerOutcome::Ok {
            result: signed_json,
        },
        signer_package: None,
    };
    driver.deliver(&serde_json::to_string(&sign_reply).unwrap());

    // ADR-0050 §D3b: a NIP-55 op reply no longer resolves the parked op on the
    // bridge thread — `deliver` sends a `DeliverSignerResponse` command, and the
    // actor's dispatch arm fans it out to the remote handles on the actor
    // thread. Model that here: pull the command and apply it to the handle (the
    // same `deliver_to_remote_signers` → `deliver_response` path the kernel
    // runs).
    match rx.try_recv().expect("DeliverSignerResponse command sent") {
        ActorCommand::Identity(IdentityCommand::DeliverSignerResponse { response_json }) => {
            handle.deliver_response(&response_json);
        }
        other => panic!("expected DeliverSignerResponse, got {other:?}"),
    }

    let resolved = op
        .wait(Duration::from_secs(2))
        .expect("sign op resolves with the Amber-signed event");
    assert_eq!(resolved.unsigned.pubkey, keys.public_key().to_hex());
    assert_eq!(resolved.unsigned.content, "loop proof");
}

/// Sign the requested unsigned-event payload with `keys` and return the
/// signed-event JSON, exactly as Amber would.
fn amber_sign(keys: &nostr::Keys, payload: &str) -> String {
    use nostr::JsonUtil;
    let v: serde_json::Value = serde_json::from_str(payload).expect("payload is JSON");
    let kind = nostr::Kind::from_u16(u16::try_from(v["kind"].as_u64().unwrap_or(1)).unwrap());
    let event = nostr::EventBuilder::new(kind, v["content"].as_str().unwrap_or_default())
        .custom_created_at(nostr::Timestamp::from(
            v["created_at"].as_u64().unwrap_or_default(),
        ))
        .sign_with_keys(keys)
        .expect("sign with test key");
    event.as_json()
}

#[test]
fn rejected_connect_reports_failed_state() {
    let _g = SERIAL.lock().unwrap();
    DISPATCHED.lock().unwrap().clear();
    let (tx, rx) = cmd_channel();
    let driver = make_driver(tx);

    driver.signin(None);
    let _awaiting = rx.try_recv().expect("awaiting state");
    let connect_request = last_dispatched_request();

    let reply = ExternalSignerResponse {
        correlation_id: connect_request.correlation_id,
        outcome: ExternalSignerOutcome::Rejected {
            reason: "user cancelled".to_string(),
        },
        signer_package: None,
    };
    driver.deliver(&serde_json::to_string(&reply).unwrap());

    match rx.try_recv().expect("failed state sent") {
        ActorCommand::Identity(IdentityCommand::Nip55SignerStateChanged { state, reason }) => {
            assert_eq!(state, "failed");
            assert_eq!(reason.as_deref(), Some("user cancelled"));
        }
        other => panic!("expected failed state, got {other:?}"),
    }
    assert!(rx.try_recv().is_err(), "no AddSigner on a rejected connect");
}

#[test]
fn malformed_response_is_dropped_silently() {
    let _g = SERIAL.lock().unwrap();
    let (tx, rx) = cmd_channel();
    let driver = make_driver(tx);
    driver.deliver("not-json");
    assert!(rx.try_recv().is_err(), "malformed response sends nothing");
}

#[test]
fn nmp_app_new_installs_restore_hook_before_explicit_init() {
    let _g = SERIAL.lock().unwrap();
    let app = crate::nmp_app_new();
    assert!(!app.is_null(), "app handle must be allocated");
    let app_ref = app_ref(app).expect("app_ref");

    assert!(
        app_ref.external_signer_driver().is_some(),
        "nmp_app_new must install the NIP-55 driver before Start"
    );
    assert!(
        app_ref.invoke_external_signer_restore_hook_for_test("not-json"),
        "restore hook must be callable before an explicit nmp_external_signer_init"
    );

    crate::nmp_app_free(app);
}

#[test]
fn restore_reconstructs_signer_without_interaction() {
    let _g = SERIAL.lock().unwrap();
    DISPATCHED.lock().unwrap().clear();
    let (tx, rx) = cmd_channel();
    let driver = make_driver(tx);

    let keys = nostr::Keys::generate();
    // `SignerPayload` serde form: `{"kind":"nip55","body":{…}}` — the
    // exact JSON `persistence_payload_json()` produced and the actor
    // persisted through the keyring capability.
    let payload = serde_json::json!({
        "kind": "nip55",
        "body": {
            "user_pubkey_hex": keys.public_key().to_hex(),
            "signer_package": "com.greenart7c3.nostrsigner",
            "granted_permissions": ["sign_event:1", "nip44_encrypt"],
        },
    });
    driver.restore(&payload.to_string());

    match rx.try_recv().expect("AddSigner sent") {
        ActorCommand::Identity(IdentityCommand::AddSigner {
            source: nmp_core::SignerSource::RemoteHandle(handle),
            make_active,
        }) => {
            assert!(make_active);
            assert_eq!(handle.signer_kind(), "nip55");
            assert_eq!(handle.pubkey_hex(), keys.public_key().to_hex());
        }
        other => panic!("expected AddSigner, got {other:?}"),
    }
    assert!(
        DISPATCHED.lock().unwrap().is_empty(),
        "restore must not require a host round-trip"
    );
}
