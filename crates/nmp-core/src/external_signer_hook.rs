//! Pluggable hook for NIP-55 external-signer session restore (ADR-0048 D4).
//! Registered by app/FFI composition at app init via
//! [`register_external_signer_hook`]; invoked by the actor's cold-start
//! session restore when the persisted active-signer kind is `"nip55"`.
//!
//! Keeps `nmp-core` ignorant of NIP-55 protocol details (D0): the kernel
//! knows there is *something* on the other side that can reconstruct a
//! remote signer from an opaque payload, but it does not name
//! `nmp-signers` or any NIP-55 type. Mirrors [`crate::bunker_hook`] — the
//! ADR-0031 worker-feeds-actor indirection precedent.
//!
//! ## Threading model
//!
//! The hook is invoked from the actor thread. The driver's implementation
//! MUST be cheap: NIP-55 restore has no handshake (the payload is
//! pubkey-only), so the hook synchronously builds the signer and enqueues
//! `ActorCommand::AddSigner` back onto the actor channel.
//!
//! ## Registration semantics
//!
//! Mirror of the bunker hook: exactly one hook, latest registration wins,
//! no unregister path. A missing hook degrades to a `last_error_toast`
//! (D6), never a panic.

use std::sync::{Arc, OnceLock, RwLock};

/// Opaque NIP-55 driver request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExternalSignerHookRequest {
    /// Restore a previously connected NIP-55 signer from the opaque
    /// pubkey-only payload the actor persisted (`SignerPayload::Nip55`).
    Restore { payload_json: String },
}

/// Hook signature: receives an opaque driver request.
pub type ExternalSignerHookFn = Arc<dyn Fn(ExternalSignerHookRequest) + Send + Sync>;

static HOOK: OnceLock<RwLock<Option<ExternalSignerHookFn>>> = OnceLock::new();

/// Register the NIP-55 driver hook. Called once by the FFI adapter
/// (`nmp-ffi`'s external-signer driver init) after constructing the driver.
/// Replaces any previously-registered hook.
pub fn register_external_signer_hook(hook: ExternalSignerHookFn) {
    let slot = HOOK.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = slot.write() {
        *guard = Some(hook);
    }
}

/// Crate-internal: restore a NIP-55 signer from opaque payload. Returns
/// `true` if a hook was registered (and called); `false` otherwise so the
/// caller can surface a fallback toast.
pub(crate) fn invoke_external_signer_restore_hook(payload_json: &str) -> bool {
    let Some(slot) = HOOK.get() else {
        return false;
    };
    let Ok(guard) = slot.read() else {
        return false;
    };
    let Some(hook) = guard.as_ref() else {
        return false;
    };
    let hook = Arc::clone(hook);
    // Drop the read lock before calling the hook — the driver may, in
    // theory, re-register from inside its handler; avoid deadlock.
    drop(guard);
    hook(ExternalSignerHookRequest::Restore {
        payload_json: payload_json.to_string(),
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // `HOOK` is process-wide static state — assert the full surface
    // (register → invoke → replace) in one test, like the bunker hook.
    #[test]
    fn register_invoke_replace() {
        let calls_a: Arc<Mutex<Vec<ExternalSignerHookRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let calls_a_clone = Arc::clone(&calls_a);
        register_external_signer_hook(Arc::new(move |request| {
            calls_a_clone.lock().unwrap().push(request);
        }));
        assert!(invoke_external_signer_restore_hook("payload-a"));
        assert_eq!(
            calls_a.lock().unwrap().as_slice(),
            &[ExternalSignerHookRequest::Restore {
                payload_json: "payload-a".to_string()
            }]
        );

        // Replace — latest registration wins.
        let calls_b: Arc<Mutex<Vec<ExternalSignerHookRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let calls_b_clone = Arc::clone(&calls_b);
        register_external_signer_hook(Arc::new(move |request| {
            calls_b_clone.lock().unwrap().push(request);
        }));
        assert!(invoke_external_signer_restore_hook("payload-b"));
        assert_eq!(calls_b.lock().unwrap().len(), 1);
        assert_eq!(calls_a.lock().unwrap().len(), 1);
    }
}
