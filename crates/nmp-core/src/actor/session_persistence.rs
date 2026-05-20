//! Actor-owned identity session persistence.
//!
//! Rust owns the policy: when a signer becomes active, persist enough material
//! through the keyring capability to restore it on the next launch. Native code
//! only executes the keychain request.

use crate::capability_socket::{dispatch_capability, CapabilityCallbackSlot};
use crate::kernel::Kernel;
use crate::relay::OutboundMessage;
use crate::substrate::{
    CapabilityEnvelope, KeyringIdentityWiring, KeyringResult, KeyringStatus, MALFORMED_RESULT,
};

use super::commands::{self, IdentityRuntime};

const ACTIVE_ACCOUNT_ID: &str = "nmp.identity.active.id";
const ACTIVE_SIGNER_KIND_ID: &str = "nmp.identity.active.kind";
const LOCAL_SECRET_PREFIX: &str = "nmp.identity.local_nsec.";
const REMOTE_SIGNER_PAYLOAD_PREFIX: &str = "nmp.identity.remote_payload.";

pub(super) fn restore_active_session(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    capability_callback: &CapabilityCallbackSlot,
    relays_ready: bool,
) -> Vec<OutboundMessage> {
    if identity.active_pubkey().is_some() {
        return Vec::new();
    }
    let active_kind = run_keyring(
        capability_callback,
        KeyringIdentityWiring::recall_secret("identity.restore.active_kind", ACTIVE_SIGNER_KIND_ID),
    );
    let KeyringResult {
        status: KeyringStatus::Ok,
        secret: Some(kind),
        ..
    } = active_kind
    else {
        return Vec::new();
    };

    let active_id = run_keyring(
        capability_callback,
        KeyringIdentityWiring::recall_secret("identity.restore.active_id", ACTIVE_ACCOUNT_ID),
    );
    let KeyringResult {
        status: KeyringStatus::Ok,
        secret: Some(identity_id),
        ..
    } = active_id
    else {
        forget_active_pointer(capability_callback);
        return Vec::new();
    };

    if kind == "local" {
        return restore_local(
            identity,
            kernel,
            capability_callback,
            relays_ready,
            &identity_id,
        );
    }
    if kind == "nip46" {
        return restore_remote_bunker(kernel, capability_callback, &identity_id);
    }
    forget_active_pointer(capability_callback);
    Vec::new()
}

fn restore_local(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    capability_callback: &CapabilityCallbackSlot,
    relays_ready: bool,
    identity_id: &str,
) -> Vec<OutboundMessage> {
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
    if identity.active_pubkey().as_deref() == Some(identity_id) {
        persist_current_active_session(identity, capability_callback);
    } else {
        forget_account(identity_id, capability_callback);
        forget_active_pointer(capability_callback);
    }
    outbound
}

fn restore_remote_bunker(
    kernel: &mut Kernel,
    capability_callback: &CapabilityCallbackSlot,
    identity_id: &str,
) -> Vec<OutboundMessage> {
    let payload = run_keyring(
        capability_callback,
        KeyringIdentityWiring::recall_secret(
            "identity.restore.remote_payload",
            remote_signer_payload_account_id(identity_id),
        ),
    );
    let KeyringResult {
        status: KeyringStatus::Ok,
        secret: Some(payload),
        ..
    } = payload
    else {
        forget_active_pointer(capability_callback);
        return Vec::new();
    };
    commands::restore_bunker_session(kernel, &payload);
    Vec::new()
}

pub(super) fn persist_current_active_session(
    identity: &IdentityRuntime,
    capability_callback: &CapabilityCallbackSlot,
) {
    let Some(identity_id) = identity.active_pubkey() else {
        forget_active_pointer(capability_callback);
        return;
    };
    match identity.active_signer_kind() {
        Some("local") => persist_active_local(identity, capability_callback, &identity_id),
        Some("nip46") => persist_active_pointer(capability_callback, &identity_id, "nip46"),
        _ => forget_active_pointer(capability_callback),
    }
}

fn persist_active_local(
    identity: &IdentityRuntime,
    capability_callback: &CapabilityCallbackSlot,
    identity_id: &str,
) {
    let Some(secret) = identity.active_nsec_bech32() else {
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
    persist_active_pointer(capability_callback, identity_id, "local");
}

pub(super) fn persist_remote_signer_payload(
    identity_id: &str,
    payload_json: &str,
    capability_callback: &CapabilityCallbackSlot,
) {
    let _ = run_keyring(
        capability_callback,
        KeyringIdentityWiring::persist_secret(
            "identity.persist.remote_payload",
            remote_signer_payload_account_id(identity_id),
            payload_json,
        ),
    );
}

pub(super) fn persist_active_pointer(
    capability_callback: &CapabilityCallbackSlot,
    identity_id: &str,
    signer_kind: &str,
) {
    let _ = run_keyring(
        capability_callback,
        KeyringIdentityWiring::persist_secret(
            "identity.persist.active_id",
            ACTIVE_ACCOUNT_ID,
            identity_id,
        ),
    );
    let _ = run_keyring(
        capability_callback,
        KeyringIdentityWiring::persist_secret(
            "identity.persist.active_kind",
            ACTIVE_SIGNER_KIND_ID,
            signer_kind,
        ),
    );
}

pub(super) fn forget_account(identity_id: &str, capability_callback: &CapabilityCallbackSlot) {
    let _ = run_keyring(
        capability_callback,
        KeyringIdentityWiring::forget_secret(
            "identity.forget.local_nsec",
            local_secret_account_id(identity_id),
        ),
    );
    let _ = run_keyring(
        capability_callback,
        KeyringIdentityWiring::forget_secret(
            "identity.forget.remote_payload",
            remote_signer_payload_account_id(identity_id),
        ),
    );
}

fn forget_active_pointer(capability_callback: &CapabilityCallbackSlot) {
    let _ = run_keyring(
        capability_callback,
        KeyringIdentityWiring::forget_secret("identity.forget.active_id", ACTIVE_ACCOUNT_ID),
    );
    let _ = run_keyring(
        capability_callback,
        KeyringIdentityWiring::forget_secret("identity.forget.active_kind", ACTIVE_SIGNER_KIND_ID),
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

fn remote_signer_payload_account_id(identity_id: &str) -> String {
    format!("{REMOTE_SIGNER_PAYLOAD_PREFIX}{identity_id}")
}
