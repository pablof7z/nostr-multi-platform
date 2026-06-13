//! Pluggable hook for `bunker://` URI handling. Installed by app/FFI
//! composition at app init into a **per-app** [`BunkerHookSlot`]; invoked by
//! the actor's `sign_in_bunker` after shape-validation succeeds.
//!
//! Keeps `nmp-core` ignorant of NIP-46 protocol details (D0 spirit): the
//! kernel knows there is *something* on the other side that will handle the
//! URI, but it does not name `nmp-signers`, `nmp-signer-broker`, or any
//! NIP-46 type.
//!
//! ## Per-app slot — no process-global (ADR-0052 §D3)
//!
//! The hook lives in an `Arc<Mutex<Option<BunkerHookFn>>>` slot created in
//! `nmp_app_new` and dropped in `nmp_app_free`, mirroring the ADR-0051
//! relay-connected hook slot. Two `NmpApp`s in one process therefore have two
//! independent hooks, and a freed-then-recreated app re-installs cleanly (the
//! Android process-reuse failure mode the old `OnceLock` global dead-ended on).
//! The actor's [`crate::actor::commands::IdentityRuntime`] owns one `Arc` clone
//! of the slot; the FFI composition root (`nmp_signer_broker_init`) holds the
//! other and writes the broker hook into it.
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
//! - At most one hook per app slot. Calling [`install_bunker_hook`] again
//!   replaces the previous registration (latest-install-wins).
//! - If no hook is installed when `sign_in_bunker` runs, the actor falls
//!   back to a `last_error_toast` indicating the broker is not initialised
//!   (D6). This is a defence against init-order bugs; in normal flow the
//!   broker is installed at startup before any URI submission can reach the
//!   actor.

use std::sync::{Arc, Mutex};

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
/// Wrapped in `Arc` so the install site can keep its own handle.
pub type BunkerHookFn = Arc<dyn Fn(BunkerHookRequest) + Send + Sync>;

/// Per-app slot holding the optional installed bunker hook.
///
/// `Arc<Mutex<Option<…>>>` so the slot is host-mutable during app construction
/// without `&mut self`, and the inner `Arc` hook can be cloned out under the
/// lock and invoked outside it (no long-held mutex around the hook body).
/// Mirrors [`crate::substrate::RelayConnectedHookSlot`].
pub type BunkerHookSlot = Arc<Mutex<Option<BunkerHookFn>>>;

/// Construct a fresh, empty [`BunkerHookSlot`].
#[must_use]
pub fn new_bunker_hook_slot() -> BunkerHookSlot {
    Arc::new(Mutex::new(None))
}

/// Install the bunker-URI handler into a per-app slot. Called by
/// `nmp_signer_broker_init` in the FFI adapter after constructing the broker.
/// Replaces any previously-installed hook (latest-install-wins). A poisoned
/// mutex recovers via `into_inner` rather than panicking the caller (D6).
pub fn install_bunker_hook(slot: &BunkerHookSlot, hook: BunkerHookFn) {
    let mut guard = slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = Some(hook);
}

/// Invoke the installed hook (if any) in `slot`. Returns `true` if a hook was
/// installed (and was called); `false` otherwise so the caller can surface a
/// fallback toast.
fn invoke_bunker_hook(slot: &BunkerHookSlot, request: BunkerHookRequest) -> bool {
    let hook = {
        let guard = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Clone the `Arc` out under the lock, then drop the guard before
        // calling the hook — the broker may, in theory, re-install from inside
        // its handler, and we don't want to deadlock.
        match guard.as_ref() {
            Some(hook) => Arc::clone(hook),
            None => return false,
        }
    };
    hook(request);
    true
}

/// Start a fresh `bunker://` connect handshake against the app's slot.
pub(crate) fn invoke_bunker_connect_hook(slot: &BunkerHookSlot, uri: &str) -> bool {
    invoke_bunker_hook(
        slot,
        BunkerHookRequest::Connect {
            uri: uri.to_string(),
        },
    )
}

/// Restore a handshaken remote signer from opaque payload against the app's
/// slot.
pub(crate) fn invoke_bunker_restore_hook(slot: &BunkerHookSlot, payload_json: &str) -> bool {
    invoke_bunker_hook(
        slot,
        BunkerHookRequest::Restore {
            payload_json: payload_json.to_string(),
        },
    )
}

/// Test-support: invoke a slot's connect hook from outside the crate (the
/// rung-5.3 per-app isolation oracle in `nmp-testing`). Production invokes via
/// the actor's `IdentityRuntime`; this exposes the same per-slot call so an
/// integration test can prove the slot is per-app without standing up the wire.
#[cfg(any(test, feature = "test-support"))]
pub fn invoke_bunker_connect_hook_for_test(slot: &BunkerHookSlot, uri: &str) -> bool {
    invoke_bunker_connect_hook(slot, uri)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn install_invoke_replace_is_per_slot() {
        let slot = new_bunker_hook_slot();

        let calls_a: Arc<Mutex<Vec<BunkerHookRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let calls_a_clone = Arc::clone(&calls_a);
        install_bunker_hook(
            &slot,
            Arc::new(move |request| {
                calls_a_clone.lock().unwrap().push(request);
            }),
        );
        assert!(invoke_bunker_connect_hook(&slot, "bunker://aaa"));
        assert_eq!(
            calls_a.lock().unwrap().as_slice(),
            &[BunkerHookRequest::Connect {
                uri: "bunker://aaa".to_string()
            }]
        );

        // Replace.
        let calls_b: Arc<Mutex<Vec<BunkerHookRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let calls_b_clone = Arc::clone(&calls_b);
        install_bunker_hook(
            &slot,
            Arc::new(move |request| {
                calls_b_clone.lock().unwrap().push(request);
            }),
        );
        assert!(invoke_bunker_restore_hook(&slot, "payload"));
        assert_eq!(
            calls_b.lock().unwrap().as_slice(),
            &[BunkerHookRequest::Restore {
                payload_json: "payload".to_string()
            }]
        );
        // Old hook is not called after replacement.
        assert_eq!(calls_a.lock().unwrap().len(), 1);
    }

    #[test]
    fn two_slots_are_independent() {
        let slot_a = new_bunker_hook_slot();
        let slot_b = new_bunker_hook_slot();
        let calls_a: Arc<Mutex<Vec<BunkerHookRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let calls_b: Arc<Mutex<Vec<BunkerHookRequest>>> = Arc::new(Mutex::new(Vec::new()));
        {
            let c = Arc::clone(&calls_a);
            install_bunker_hook(&slot_a, Arc::new(move |r| c.lock().unwrap().push(r)));
        }
        {
            let c = Arc::clone(&calls_b);
            install_bunker_hook(&slot_b, Arc::new(move |r| c.lock().unwrap().push(r)));
        }
        assert!(invoke_bunker_connect_hook(&slot_a, "bunker://a"));
        assert_eq!(calls_a.lock().unwrap().len(), 1);
        assert_eq!(calls_b.lock().unwrap().len(), 0);
    }

    #[test]
    fn empty_slot_returns_false() {
        let slot = new_bunker_hook_slot();
        assert!(!invoke_bunker_connect_hook(&slot, "bunker://none"));
    }
}
