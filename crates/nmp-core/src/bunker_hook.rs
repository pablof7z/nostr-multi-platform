//! Pluggable hook for `bunker://` URI handling. Registered by app/FFI
//! composition at app init via [`register_bunker_hook`]; invoked by the
//! actor's `sign_in_bunker` after shape-validation succeeds.
//!
//! Keeps `nmp-core` ignorant of NIP-46 protocol details (D0 spirit): the
//! kernel knows there is *something* on the other side that will handle the
//! URI, but it does not name `nmp-signers`, `nmp-signer-broker`, or any
//! NIP-46 type.
//!
//! ## Threading model
//!
//! The hook is invoked from the actor thread. The broker's implementation
//! MUST be cheap (it typically dispatches the URI onto a worker thread that
//! drives the handshake out-of-band). Long-running blocking work in the hook
//! would stall the actor's message loop.
//!
//! ## Registration semantics
//!
//! - Exactly one hook is registered. Calling [`register_bunker_hook`] again
//!   replaces the previous registration. There is no formal "unregister"
//!   path — the broker is intended to be initialised once per process.
//! - If no hook is registered when `sign_in_bunker` runs, the actor falls
//!   back to a `last_error_toast` indicating the broker is not initialised.
//!   This is a defence against init-order bugs; in normal flow the broker is
//!   registered at startup before any URI submission can reach the actor.

use std::sync::{Arc, OnceLock, RwLock};

/// Opaque broker request. The actor owns session policy; the broker owns the
/// NIP-46 transport details.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BunkerHookRequest {
    /// Start a fresh `bunker://` connect handshake from user input.
    Connect { uri: String },
    /// Restore a previously handshaken remote signer from an opaque signer
    /// payload stored by the actor.
    Restore { payload_json: String },
}

/// Hook signature: receives an opaque broker request.
/// Wrapped in `Arc` so the registration site can keep its own handle.
pub type BunkerHookFn = Arc<dyn Fn(BunkerHookRequest) + Send + Sync>;

static HOOK: OnceLock<RwLock<Option<BunkerHookFn>>> = OnceLock::new();

/// Register the bunker-URI handler. Called once by `nmp_signer_broker_init`
/// in the FFI adapter after constructing the broker. Replaces any
/// previously-registered hook.
pub fn register_bunker_hook(hook: BunkerHookFn) {
    let slot = HOOK.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = slot.write() {
        *guard = Some(hook);
    }
}

/// Crate-internal: invoke the registered hook if any. Returns `true` if a
/// hook was registered (and was called); `false` otherwise so the caller can
/// surface a fallback toast.
fn invoke_bunker_hook(request: BunkerHookRequest) -> bool {
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
    // Drop the read lock before calling the hook — the broker may, in theory,
    // re-register from inside its handler, and we don't want to deadlock.
    drop(guard);
    hook(request);
    true
}

/// Crate-internal: start a fresh `bunker://` connect handshake.
pub(crate) fn invoke_bunker_connect_hook(uri: &str) -> bool {
    invoke_bunker_hook(BunkerHookRequest::Connect {
        uri: uri.to_string(),
    })
}

/// Crate-internal: restore a handshaken remote signer from opaque payload.
pub(crate) fn invoke_bunker_restore_hook(payload_json: &str) -> bool {
    invoke_bunker_hook(BunkerHookRequest::Restore {
        payload_json: payload_json.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // NOTE: `HOOK` is process-wide static state. These tests run serially on
    // a single global slot; resetting between tests is not possible (OnceLock
    // is fire-once). We instead assert the latest-registration-wins semantics
    // in a single test that exercises the full surface.
    #[test]
    fn register_invoke_replace() {
        let calls_a: Arc<Mutex<Vec<BunkerHookRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let calls_a_clone = Arc::clone(&calls_a);
        register_bunker_hook(Arc::new(move |request| {
            calls_a_clone.lock().unwrap().push(request);
        }));
        assert!(invoke_bunker_connect_hook("bunker://aaa"));
        assert_eq!(
            calls_a.lock().unwrap().as_slice(),
            &[BunkerHookRequest::Connect {
                uri: "bunker://aaa".to_string()
            }]
        );

        // Replace.
        let calls_b: Arc<Mutex<Vec<BunkerHookRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let calls_b_clone = Arc::clone(&calls_b);
        register_bunker_hook(Arc::new(move |request| {
            calls_b_clone.lock().unwrap().push(request);
        }));
        assert!(invoke_bunker_restore_hook("payload"));
        assert_eq!(
            calls_b.lock().unwrap().as_slice(),
            &[BunkerHookRequest::Restore {
                payload_json: "payload".to_string()
            }]
        );
        // Old hook is not called after replacement.
        assert_eq!(calls_a.lock().unwrap().len(), 1);
    }
}
