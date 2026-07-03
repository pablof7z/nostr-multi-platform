//! Actor-owned identity session persistence.
//!
//! Rust owns the policy: when a signer becomes active, persist enough material
//! through the keyring capability to restore it on the next launch. Native code
//! only executes the keychain request.
//!
//! ## ADR-0072 §3 — V-90 Site 2: write path moves off the actor thread
//!
//! The write functions (`persist_*`, `forget_*`) previously called
//! `dispatch_capability` synchronously on the actor thread, blocking it for
//! hundreds of ms on every account sign-in / switch / remove (iOS Keychain
//! hit). They now enqueue a [`super::capability_worker::CapabilityWorkItem`]
//! to the serialized capability worker and return immediately. The worker runs
//! `dispatch_capability` off-actor and re-enters via
//! `ActorCommand::CapabilityResultReady`; the dispatch arm applies the result
//! (error → toast, absent account → D6 trace + drop).
//!
//! ## What stays synchronous
//!
//! `restore_active_session` (and its read sub-functions) remain synchronous.
//! This is the cold-start recall chain (`Start` arm only): each recall drives
//! the next step as a sequential continuation. Converting it to fire-and-forget
//! would require a multi-tick state machine the ADR never designed for this
//! path. Cold-start runs at most once per process, is below the liveness
//! threshold (ADR-0072: "hundreds of ms — short of the liveness threshold"),
//! and is not among the per-switch hitches GH #613 targets. The `Start` arm
//! also calls `persist_current_active_session` after restore — that call now
//! goes through the enqueue path, so the tail-write is also off-actor.

use crate::capability_socket::{dispatch_capability, CapabilityCallbackSlot};
use crate::kernel::Kernel;
use crate::relay::OutboundMessage;
use crate::substrate::{
    CapabilityEnvelope, KeyringIdentityWiring, KeyringResult, KeyringStatus, MALFORMED_RESULT,
};

use super::capability_worker::{make_work_item, CapabilityWorkSender};
use super::commands::{self, IdentityRuntime};

const ACTIVE_ACCOUNT_ID: &str = "nmp.identity.active.id";
const ACTIVE_SIGNER_KIND_ID: &str = "nmp.identity.active.kind";
const APP_MANAGED_LOCAL_IDS: &str = "nmp.identity.app_managed.local_ids";
const LOCAL_SECRET_PREFIX: &str = "nmp.identity.local_nsec.";
const REMOTE_SIGNER_PAYLOAD_PREFIX: &str = "nmp.identity.remote_payload.";

// ─────────────────────────────────────────────────────────────────────────────
// Read path (synchronous — cold-start only, `Start` arm)
// ─────────────────────────────────────────────────────────────────────────────

/// Restore the previously-persisted session on cold start (called from the
/// actor's `Start` arm). **Synchronous** — reads drive sign-in continuations
/// that cannot be deferred. See the module-level rationale.
pub(super) fn restore_active_session(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    capability_callback: &CapabilityCallbackSlot,
    capability_work_tx: &CapabilityWorkSender,
    relays_ready: bool,
) -> Vec<OutboundMessage> {
    restore_app_managed_local_signers(identity, kernel, capability_callback, capability_work_tx);
    if let Some(active) = identity.active_pubkey() {
        if kernel.active_account_pubkey() != Some(active.as_str()) {
            commands::sync_kernel(identity, kernel);
        }
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
        enqueue_forget_active_pointer(capability_work_tx, ACTIVE_ACCOUNT_ID);
        return Vec::new();
    };

    if kind == "local" {
        return restore_local(
            identity,
            kernel,
            capability_callback,
            capability_work_tx,
            relays_ready,
            &identity_id,
        );
    }
    if kind == "nip46" || kind == "nip55" {
        return restore_remote_signer(
            identity,
            kernel,
            capability_callback,
            capability_work_tx,
            &identity_id,
            &kind,
        );
    }
    enqueue_forget_active_pointer(capability_work_tx, ACTIVE_ACCOUNT_ID);
    Vec::new()
}

fn restore_local(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    capability_callback: &CapabilityCallbackSlot,
    capability_work_tx: &CapabilityWorkSender,
    relays_ready: bool,
    identity_id: &str,
) -> Vec<OutboundMessage> {
    let secret = run_keyring(
        capability_callback,
        KeyringIdentityWiring::recall_secret(
            "identity.restore.local_nsec",
            local_secret_account_id(identity_id),
        ),
    );
    let KeyringResult {
        status: KeyringStatus::Ok,
        secret: Some(secret),
        ..
    } = secret
    else {
        enqueue_forget_active_pointer(capability_work_tx, identity_id);
        return Vec::new();
    };

    // Restoring a persisted active account: add the local key and make it
    // active (mirrors the pre-`AddSigner` `sign_in_nsec`, which always
    // activated). The secret is wrapped in `Zeroizing` so the plaintext is
    // wiped when `add_signer` drops the source.
    let outbound = commands::add_signer(
        identity,
        kernel,
        crate::actor::SignerSource::LocalNsec(zeroize::Zeroizing::new(secret)),
        true,
        relays_ready,
    );
    if identity.active_pubkey().as_deref() == Some(identity_id) {
        enqueue_persist_current_active_session(identity, capability_work_tx);
    } else {
        enqueue_forget_account(identity_id, capability_work_tx);
        enqueue_forget_active_pointer(capability_work_tx, identity_id);
    }
    outbound
}

fn restore_app_managed_local_signers(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    capability_callback: &CapabilityCallbackSlot,
    capability_work_tx: &CapabilityWorkSender,
) {
    let roster = run_keyring(
        capability_callback,
        KeyringIdentityWiring::recall_secret(
            "identity.restore.app_managed_local_ids",
            APP_MANAGED_LOCAL_IDS,
        ),
    );
    let KeyringResult {
        status: KeyringStatus::Ok,
        secret: Some(roster_json),
        ..
    } = roster
    else {
        return;
    };
    let Ok(ids) = serde_json::from_str::<Vec<String>>(&roster_json) else {
        enqueue_write(
            capability_work_tx,
            APP_MANAGED_LOCAL_IDS,
            KeyringIdentityWiring::forget_secret(
                "identity.forget.app_managed_local_ids",
                APP_MANAGED_LOCAL_IDS,
            ),
        );
        return;
    };
    for identity_id in ids {
        let secret = run_keyring(
            capability_callback,
            KeyringIdentityWiring::recall_secret(
                "identity.restore.app_managed_local_nsec",
                local_secret_account_id(&identity_id),
            ),
        );
        let KeyringResult {
            status: KeyringStatus::Ok,
            secret: Some(secret),
            ..
        } = secret
        else {
            enqueue_forget_account(&identity_id, capability_work_tx);
            continue;
        };
        commands::add_signer(
            identity,
            kernel,
            crate::actor::SignerSource::AppManagedLocalNsec(zeroize::Zeroizing::new(secret)),
            false,
            false,
        );
    }
    enqueue_persist_app_managed_local_signers(identity, capability_work_tx);
}

fn restore_remote_signer(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    capability_callback: &CapabilityCallbackSlot,
    capability_work_tx: &CapabilityWorkSender,
    identity_id: &str,
    kind: &str,
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
        enqueue_forget_active_pointer(capability_work_tx, identity_id);
        return Vec::new();
    };
    // Both restores route through opaque payloads + registered hooks (D0):
    // NIP-46 re-handshakes via the broker; NIP-55 reconstructs synchronously
    // via the external-signer driver (ADR-0072 D4 — pubkey-only payload).
    if kind == "nip55" {
        commands::restore_nip55_session(identity, kernel, &payload);
    } else {
        commands::restore_bunker_session(identity, kernel, &payload);
    }
    Vec::new()
}

// ─────────────────────────────────────────────────────────────────────────────
// Write path (async — enqueue to capability worker, D8)
//
// All functions below build their `CapabilityRequest` on the actor thread
// (reading actor-owned `IdentityRuntime` / plain string data) and enqueue
// the pre-serialized item to the worker. They return immediately; the worker
// runs `dispatch_capability` off-actor and re-enters via
// `ActorCommand::CapabilityResultReady`. The actor's dispatch arm applies
// error results (toast) and drops results for removed accounts (D6 trace).
// ─────────────────────────────────────────────────────────────────────────────

/// Persist the current active session through the capability worker.
/// Reads identity state on-actor, serializes the request, and enqueues.
pub(super) fn enqueue_persist_current_active_session(
    identity: &IdentityRuntime,
    capability_work_tx: &CapabilityWorkSender,
) {
    let Some(identity_id) = identity.active_pubkey() else {
        enqueue_forget_active_pointer(capability_work_tx, ACTIVE_ACCOUNT_ID);
        return;
    };
    match identity.active_signer_kind() {
        Some("local") => enqueue_persist_active_local(identity, capability_work_tx, &identity_id),
        Some(kind @ ("nip46" | "nip55")) => {
            enqueue_persist_active_pointer(capability_work_tx, &identity_id, kind)
        }
        _ => enqueue_forget_active_pointer(capability_work_tx, ACTIVE_ACCOUNT_ID),
    }
}

fn enqueue_persist_active_local(
    identity: &IdentityRuntime,
    capability_work_tx: &CapabilityWorkSender,
    identity_id: &str,
) {
    let Some(secret) = identity.active_nsec_bech32() else {
        enqueue_forget_active_pointer(capability_work_tx, ACTIVE_ACCOUNT_ID);
        return;
    };
    enqueue_write(
        capability_work_tx,
        identity_id,
        KeyringIdentityWiring::persist_secret(
            "identity.persist.local_nsec",
            local_secret_account_id(identity_id),
            secret,
        ),
    );
    enqueue_persist_active_pointer(capability_work_tx, identity_id, "local");
}

pub(super) fn enqueue_persist_app_managed_local_signers(
    identity: &IdentityRuntime,
    capability_work_tx: &CapabilityWorkSender,
) {
    let entries = identity.app_managed_local_secrets();
    let owner_id = entries
        .first()
        .map(|(id, _)| id.clone())
        .or_else(|| identity.active_pubkey())
        .unwrap_or_else(|| APP_MANAGED_LOCAL_IDS.to_string());
    for (identity_id, secret) in &entries {
        enqueue_write(
            capability_work_tx,
            identity_id,
            KeyringIdentityWiring::persist_secret(
                "identity.persist.app_managed_local_nsec",
                local_secret_account_id(identity_id),
                secret.clone(),
            ),
        );
    }
    let ids = entries
        .iter()
        .map(|(identity_id, _)| identity_id.clone())
        .collect::<Vec<_>>();
    let roster_json = serde_json::to_string(&ids).unwrap_or_else(|_| "[]".to_string());
    enqueue_write(
        capability_work_tx,
        &owner_id,
        KeyringIdentityWiring::persist_secret(
            "identity.persist.app_managed_local_ids",
            APP_MANAGED_LOCAL_IDS,
            roster_json,
        ),
    );
}

pub(super) fn enqueue_persist_remote_signer_payload(
    identity_id: &str,
    payload_json: &str,
    capability_work_tx: &CapabilityWorkSender,
) {
    enqueue_write(
        capability_work_tx,
        identity_id,
        KeyringIdentityWiring::persist_secret(
            "identity.persist.remote_payload",
            remote_signer_payload_account_id(identity_id),
            payload_json,
        ),
    );
}

pub(super) fn enqueue_persist_active_pointer(
    capability_work_tx: &CapabilityWorkSender,
    identity_id: &str,
    signer_kind: &str,
) {
    enqueue_write(
        capability_work_tx,
        identity_id,
        KeyringIdentityWiring::persist_secret(
            "identity.persist.active_id",
            ACTIVE_ACCOUNT_ID,
            identity_id,
        ),
    );
    enqueue_write(
        capability_work_tx,
        identity_id,
        KeyringIdentityWiring::persist_secret(
            "identity.persist.active_kind",
            ACTIVE_SIGNER_KIND_ID,
            signer_kind,
        ),
    );
}

pub(super) fn enqueue_forget_account(identity_id: &str, capability_work_tx: &CapabilityWorkSender) {
    enqueue_write(
        capability_work_tx,
        identity_id,
        KeyringIdentityWiring::forget_secret(
            "identity.forget.local_nsec",
            local_secret_account_id(identity_id),
        ),
    );
    enqueue_write(
        capability_work_tx,
        identity_id,
        KeyringIdentityWiring::forget_secret(
            "identity.forget.remote_payload",
            remote_signer_payload_account_id(identity_id),
        ),
    );
}

fn enqueue_forget_active_pointer(capability_work_tx: &CapabilityWorkSender, account_id: &str) {
    enqueue_write(
        capability_work_tx,
        account_id,
        KeyringIdentityWiring::forget_secret("identity.forget.active_id", ACTIVE_ACCOUNT_ID),
    );
    enqueue_write(
        capability_work_tx,
        account_id,
        KeyringIdentityWiring::forget_secret("identity.forget.active_kind", ACTIVE_SIGNER_KIND_ID),
    );
}

/// Enqueue a single capability write item to the worker. Called on the actor
/// thread only; never blocks. A send failure (disconnected channel) is
/// silently dropped — the channel only disconnects on actor teardown, at
/// which point writes are irrelevant (D6).
fn enqueue_write(
    capability_work_tx: &CapabilityWorkSender,
    account_id: &str,
    request: crate::substrate::CapabilityRequest,
) {
    if let Some(item) = make_work_item(account_id, &request) {
        // D6 — silently drop on disconnected channel (actor teardown).
        let _ = capability_work_tx.send(item);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Read helpers (synchronous — only used by the restore path)
// ─────────────────────────────────────────────────────────────────────────────

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
