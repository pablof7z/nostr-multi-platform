//! Pluggable hook for NIP-55 external-signer session restore (ADR-0048 D4).
//! Installed by app/FFI composition at app init into a **per-app**
//! [`ExternalSignerHookSlot`]; invoked by the actor's cold-start session
//! restore when the persisted active-signer kind is `"nip55"`.
//!
//! Keeps `nmp-core` ignorant of NIP-55 protocol details (D0): the kernel
//! knows there is *something* on the other side that can reconstruct a
//! remote signer from an opaque payload, but it does not name
//! `nmp-signers` or any NIP-55 type. Structural twin of [`crate::bunker_hook`]
//! — the ADR-0031 worker-feeds-actor indirection precedent.
//!
//! ## Per-app slot — no process-global (ADR-0052 §D3)
//!
//! Like the bunker hook, this lives in an `Arc<Mutex<Option<…>>>` slot created
//! in `nmp_app_new` and dropped in `nmp_app_free` (mirroring ADR-0051's
//! relay-connected hook slot). Two apps get two independent hooks; a
//! freed-then-recreated app re-installs cleanly. The actor's `IdentityRuntime`
//! owns one `Arc` clone; the FFI `nmp_external_signer_init` holds the other and
//! installs the driver's restore hook into it.
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
//! Mirror of the bunker hook: at most one hook per app slot, latest-install
//! wins. A missing hook degrades to a `last_error_toast` (D6), never a panic.

use std::sync::{Arc, Mutex};

/// Opaque NIP-55 driver request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExternalSignerHookRequest {
    /// Restore a previously connected NIP-55 signer from the opaque
    /// pubkey-only payload the actor persisted (`SignerPayload::Nip55`).
    Restore { payload_json: String },
}

/// Hook signature: receives an opaque driver request.
pub type ExternalSignerHookFn = Arc<dyn Fn(ExternalSignerHookRequest) + Send + Sync>;

/// Per-app slot holding the optional installed NIP-55 restore hook. Twin of
/// [`crate::bunker_hook::BunkerHookSlot`].
pub type ExternalSignerHookSlot = Arc<Mutex<Option<ExternalSignerHookFn>>>;

/// Construct a fresh, empty [`ExternalSignerHookSlot`].
#[must_use]
pub fn new_external_signer_hook_slot() -> ExternalSignerHookSlot {
    Arc::new(Mutex::new(None))
}

/// Install the NIP-55 driver hook into a per-app slot. Called by the FFI
/// adapter (`nmp_external_signer_init`) after constructing the driver.
/// Replaces any previously-installed hook (latest-install-wins). A poisoned
/// mutex recovers via `into_inner` (D6).
pub fn install_external_signer_hook(slot: &ExternalSignerHookSlot, hook: ExternalSignerHookFn) {
    let mut guard = slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = Some(hook);
}

/// Restore a NIP-55 signer from opaque payload against the app's slot. Returns
/// `true` if a hook was installed (and called); `false` otherwise so the
/// caller can surface a fallback toast.
pub(crate) fn invoke_external_signer_restore_hook(
    slot: &ExternalSignerHookSlot,
    payload_json: &str,
) -> bool {
    let hook = {
        let guard = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Clone the `Arc` out under the lock, then drop the guard before
        // calling the hook — the driver may, in theory, re-install from inside
        // its handler; avoid deadlock.
        match guard.as_ref() {
            Some(hook) => Arc::clone(hook),
            None => return false,
        }
    };
    hook(ExternalSignerHookRequest::Restore {
        payload_json: payload_json.to_string(),
    });
    true
}

/// Test-support: invoke a slot's restore hook from outside the crate (the
/// rung-5.3 per-app isolation oracle in `nmp-testing`).
#[cfg(any(test, feature = "test-support"))]
pub fn invoke_external_signer_restore_hook_for_test(
    slot: &ExternalSignerHookSlot,
    payload_json: &str,
) -> bool {
    invoke_external_signer_restore_hook(slot, payload_json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn install_invoke_replace_is_per_slot() {
        let slot = new_external_signer_hook_slot();

        let calls_a: Arc<Mutex<Vec<ExternalSignerHookRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let calls_a_clone = Arc::clone(&calls_a);
        install_external_signer_hook(
            &slot,
            Arc::new(move |request| {
                calls_a_clone.lock().unwrap().push(request);
            }),
        );
        assert!(invoke_external_signer_restore_hook(&slot, "payload-a"));
        assert_eq!(
            calls_a.lock().unwrap().as_slice(),
            &[ExternalSignerHookRequest::Restore {
                payload_json: "payload-a".to_string()
            }]
        );

        // Replace — latest install wins.
        let calls_b: Arc<Mutex<Vec<ExternalSignerHookRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let calls_b_clone = Arc::clone(&calls_b);
        install_external_signer_hook(
            &slot,
            Arc::new(move |request| {
                calls_b_clone.lock().unwrap().push(request);
            }),
        );
        assert!(invoke_external_signer_restore_hook(&slot, "payload-b"));
        assert_eq!(calls_b.lock().unwrap().len(), 1);
        assert_eq!(calls_a.lock().unwrap().len(), 1);
    }

    #[test]
    fn two_slots_are_independent() {
        let slot_a = new_external_signer_hook_slot();
        let slot_b = new_external_signer_hook_slot();
        let calls_a: Arc<Mutex<Vec<ExternalSignerHookRequest>>> = Arc::new(Mutex::new(Vec::new()));
        {
            let c = Arc::clone(&calls_a);
            install_external_signer_hook(&slot_a, Arc::new(move |r| c.lock().unwrap().push(r)));
        }
        // slot_b has no hook installed.
        assert!(invoke_external_signer_restore_hook(&slot_a, "to-a"));
        assert!(!invoke_external_signer_restore_hook(&slot_b, "to-none"));
        assert_eq!(calls_a.lock().unwrap().len(), 1);
    }

    #[test]
    fn empty_slot_returns_false() {
        let slot = new_external_signer_hook_slot();
        assert!(!invoke_external_signer_restore_hook(&slot, "none"));
    }
}
