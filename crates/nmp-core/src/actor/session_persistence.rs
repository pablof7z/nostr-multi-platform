//! Actor-owned local identity session persistence.
//!
//! Rust owns the policy: when a local signer becomes active, persist its nsec
//! through the keyring capability; on start, restore that active local signer
//! before the first snapshot. Native code only executes the keychain request.

use crate::capability_socket::{dispatch_capability, CapabilityCallbackSlot};
use crate::kernel::Kernel;
use crate::relay::OutboundMessage;
use crate::substrate::{
    CapabilityEnvelope, KeyringIdentityWiring, KeyringResult, KeyringStatus, MALFORMED_RESULT,
};

use super::commands::{self, IdentityRuntime};

const ACTIVE_LOCAL_ACCOUNT_ID: &str = "nmp.identity.active.local";
const LOCAL_SECRET_PREFIX: &str = "nmp.identity.local_nsec.";

pub(super) fn restore_active_local(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    capability_callback: &CapabilityCallbackSlot,
    relays_ready: bool,
) -> Vec<OutboundMessage> {
    if identity.active_pubkey().is_some() {
        return Vec::new();
    }
    let active = run_keyring(
        capability_callback,
        KeyringIdentityWiring::recall_secret(
            "identity.restore.active_local",
            ACTIVE_LOCAL_ACCOUNT_ID,
        ),
    );
    let KeyringResult {
        status: KeyringStatus::Ok,
        secret: Some(identity_id),
        ..
    } = active
    else {
        return Vec::new();
    };

    let secret = run_keyring(
        capability_callback,
        KeyringIdentityWiring::recall_secret(
            "identity.restore.local_nsec",
            local_secret_account_id(&identity_id),
        ),
    );
    let KeyringResult {
        status: KeyringStatus::Ok,
        secret: Some(secret),
        ..
    } = secret
    else {
        forget_active_pointer(capability_callback);
        return Vec::new();
    };

    let outbound = commands::sign_in_nsec(identity, kernel, &secret, relays_ready);
    if identity.active_pubkey().as_deref() == Some(identity_id.as_str()) {
        persist_current_active_local(identity, capability_callback);
    } else {
        forget_local_account(&identity_id, capability_callback);
        forget_active_pointer(capability_callback);
    }
    outbound
}

pub(super) fn persist_current_active_local(
    identity: &IdentityRuntime,
    capability_callback: &CapabilityCallbackSlot,
) {
    let (Some(identity_id), Some(secret)) =
        (identity.active_pubkey(), identity.active_nsec_bech32())
    else {
        forget_active_pointer(capability_callback);
        return;
    };
    let _ = run_keyring(
        capability_callback,
        KeyringIdentityWiring::persist_secret(
            "identity.persist.local_nsec",
            local_secret_account_id(&identity_id),
            secret,
        ),
    );
    let _ = run_keyring(
        capability_callback,
        KeyringIdentityWiring::persist_secret(
            "identity.persist.active_local",
            ACTIVE_LOCAL_ACCOUNT_ID,
            identity_id,
        ),
    );
}

pub(super) fn forget_local_account(
    identity_id: &str,
    capability_callback: &CapabilityCallbackSlot,
) {
    let _ = run_keyring(
        capability_callback,
        KeyringIdentityWiring::forget_secret(
            "identity.forget.local_nsec",
            local_secret_account_id(identity_id),
        ),
    );
}

fn forget_active_pointer(capability_callback: &CapabilityCallbackSlot) {
    let _ = run_keyring(
        capability_callback,
        KeyringIdentityWiring::forget_secret(
            "identity.forget.active_local",
            ACTIVE_LOCAL_ACCOUNT_ID,
        ),
    );
}

fn run_keyring(
    capability_callback: &CapabilityCallbackSlot,
    request: crate::substrate::CapabilityRequest,
) -> KeyringResult {
    let Ok(request_json) = serde_json::to_string(&request) else {
        return KeyringResult::error(MALFORMED_RESULT);
    };
    let envelope_json = dispatch_capability(capability_callback, &request_json);
    let Ok(envelope) = serde_json::from_str::<CapabilityEnvelope>(&envelope_json) else {
        return KeyringResult::error(MALFORMED_RESULT);
    };
    KeyringIdentityWiring::decode_result(&envelope)
}

fn local_secret_account_id(identity_id: &str) -> String {
    format!("{LOCAL_SECRET_PREFIX}{identity_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability_socket::CapabilityCallbackRegistration;
    use crate::kernel::Kernel;
    use crate::relay::DEFAULT_VISIBLE_LIMIT;
    use crate::substrate::{CapabilityEnvelope, KeyringRequest};
    use std::collections::HashMap;
    use std::ffi::{c_char, c_void, CStr, CString};
    use std::sync::Mutex;

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
        (IdentityRuntime::new(), Kernel::new(DEFAULT_VISIBLE_LIMIT))
    }

    #[test]
    fn restores_imported_nsec_without_swift_cache() {
        let _g = SERIAL.lock().unwrap();
        *STORE.lock().unwrap() = Some(HashMap::new());
        let slot = registered_slot();

        let (mut identity, mut kernel) = fresh();
        commands::sign_in_nsec(&mut identity, &mut kernel, TEST_NSEC, false);
        let expected = identity.active_pubkey().unwrap();
        persist_current_active_local(&identity, &slot);

        let (mut restored_identity, mut restored_kernel) = fresh();
        restore_active_local(&mut restored_identity, &mut restored_kernel, &slot, false);

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

        let (mut identity, mut kernel) = fresh();
        commands::create_account(&mut identity, &mut kernel, false, &HashMap::new(), &[]);
        let expected = identity.active_pubkey().unwrap();
        persist_current_active_local(&identity, &slot);

        let (mut restored_identity, mut restored_kernel) = fresh();
        restore_active_local(&mut restored_identity, &mut restored_kernel, &slot, false);

        assert_eq!(restored_identity.active_pubkey(), Some(expected.clone()));
        assert_eq!(restored_kernel.account_snapshot().1, Some(&expected));
    }
}
