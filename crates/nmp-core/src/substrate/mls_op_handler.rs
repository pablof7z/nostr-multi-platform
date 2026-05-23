//! `MlsOpHandler` — substrate-generic seam for stateful, host-owned op handlers
//! that the actor invokes from a typed `ActorCommand` arm.
//!
//! # Why this exists
//!
//! `ActionModule::execute` is a *static* method whose only output is enqueuing
//! `ActorCommand`s — by design, it has no access to per-app projection state.
//! `PublishModule`'s executor encodes everything it needs into a typed
//! `ActorCommand::PublishNote { content, target, ... }` and the actor's
//! dispatch arm signs+publishes. That works because publish state lives in the
//! kernel.
//!
//! Some app crates own stateful runtime that the kernel cannot name (D0): the
//! Marmot MLS state (a per-process `MarmotService<MdkSqliteStorage>` holding
//! group ratchet secrets, processed Welcomes, key-package private keys) lives
//! in `nmp-app-marmot`, not `nmp-core`. For those crates an `ActorCommand`
//! variant per op (`ActorCommand::MarmotCreateGroup { ... }`) would force
//! `nmp-core` to name the app's nouns — exactly what D0 forbids.
//!
//! `MlsOpHandler` is the boundary-shaped seam: a small, substrate-generic
//! trait `nmp-core` defines so the actor can ask "whoever owns the MLS state,
//! run this op for me" without knowing what the op is. The host installs an
//! `Arc<dyn MlsOpHandler>` into [`NmpApp::set_mls_op_handler`]; the actor's
//! [`ActorCommand::DispatchMlsOp`] arm pulls the handler from the slot and
//! calls [`MlsOpHandler::handle`].
//!
//! # Naming (D0)
//!
//! The trait is named after the *protocol layer* (MLS / Messaging Layer
//! Security, RFC 9420 — the open IETF protocol Marmot wraps), NOT the app
//! crate that consumes it. This is the same precedent as
//! [`crate::NmpApp::mls_local_nsec`] (ADR-0025 — the raw-nsec slot is named
//! `mls_local_nsec`, not `marmot_local_nsec`). A second MLS-driven app crate
//! could install its own `MlsOpHandler` without renaming the seam.
//!
//! # The contract
//!
//! `handle` consumes a JSON op envelope (the action body's `Action` payload,
//! re-serialized to a string by the [`ActionModule::execute`] body) plus the
//! registry-minted `correlation_id`, runs the op synchronously on whatever
//! thread the actor's dispatch arm calls it from, and returns a JSON value.
//! The actor dispatch arm wires that value into a snapshot projection keyed
//! by `correlation_id` so the host can pick up the result on the next tick
//! (the same pull-model contract `register_snapshot_projection` exposes).
//!
//! No MLS / Marmot / openmls type ever crosses this boundary — the JSON
//! string and `serde_json::Value` are the only types it speaks. The handler
//! impl on the app side translates between its own typed action enum and this
//! JSON shape; `nmp-core` never sees the typed action enum.
//!
//! # D6 — no panic crosses the trait
//!
//! Implementations MUST NOT panic. The actor's `DispatchMlsOp` arm wraps the
//! call in `catch_unwind` (the same way `ActionRegistry::execute` does for
//! `ActionModule::execute`), so a panic is converted to a `Failed` action
//! stage rather than unwinding across the FFI boundary — but a well-behaved
//! impl returns an `{"ok":false,"error":...}` envelope for soft failures
//! instead of relying on the catch.
//!
//! # D8 — handlers must not block the actor thread for long
//!
//! `handle` runs *inline on the actor thread* (the same thread that drains
//! `ActorCommand`s and ticks the kernel). MLS state mutations are
//! SQLite-bound and typically sub-100ms, which is within the actor's tick
//! budget. A handler whose op routinely exceeds ~50ms SHOULD spawn a worker
//! thread internally and fan a follow-up `ActorCommand` back via the actor's
//! self-feedback sender (the same pattern `FetchLnurlInvoice` uses for the
//! LNURL HTTP round-trip — see [`crate::actor::ActorCommand::FetchLnurlInvoice`]).
//! The trait does not enforce this — it's the implementor's responsibility,
//! same as for every other `ActorCommand` dispatch arm.

use std::sync::{Arc, Mutex};

/// A host-installed handler for stateful "MLS op" actions dispatched through
/// the [`crate::kernel::ActionRegistry`].
///
/// See the module rustdoc for the full contract. The blanket `Send + Sync`
/// bound is required because the handler is stored in a shared `Arc` slot
/// that the actor thread reads.
pub trait MlsOpHandler: Send + Sync {
    /// Run one MLS op.
    ///
    /// * `action_json` — the action body serialized to JSON. The handler
    ///   parses it into its own typed action enum (the same enum the
    ///   `ActionModule::execute` body that built this command serialized
    ///   from).
    /// * `correlation_id` — the registry-minted dispatch id. The handler
    ///   includes it in the returned envelope when callers need to pair
    ///   results with the dispatch return value; it MAY be ignored for
    ///   fire-and-forget ops.
    ///
    /// Returns the op result as a `serde_json::Value` — a `{"ok":true,...}` /
    /// `{"ok":false,"error":...}` envelope by convention. The actor's
    /// `DispatchMlsOp` arm threads this value into a snapshot projection
    /// keyed by `correlation_id`.
    ///
    /// MUST NOT panic (see D6 in the module docs).
    fn handle(&self, action_json: &str, correlation_id: &str) -> serde_json::Value;
}

/// Typed slot holding the host-installed [`MlsOpHandler`].
///
/// `Arc<Mutex<Option<Arc<dyn MlsOpHandler>>>>` because:
///
/// * the outer `Arc<Mutex<...>>` is the shared-slot pattern every other
///   `NmpApp` ↔ actor slot uses ([`crate::ffi::MlsLocalNsecSlot`],
///   [`crate::ffi::Nip17LocalKeysSlot`], etc.) — the `Mutex` is what makes
///   the slot writable without `&mut self` on `NmpApp`.
/// * the inner `Arc<dyn MlsOpHandler>` is what the actor clones out under
///   the lock and calls — calling `handle` does NOT hold the outer mutex,
///   so a long-running handler does not block the FFI `set_mls_op_handler`
///   write path.
pub type MlsOpHandlerSlot = Arc<Mutex<Option<Arc<dyn MlsOpHandler>>>>;

/// Construct a fresh, empty [`MlsOpHandlerSlot`].
pub fn new_mls_op_handler_slot() -> MlsOpHandlerSlot {
    Arc::new(Mutex::new(None))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trivial handler used to exercise the trait shape directly.
    struct EchoHandler;
    impl MlsOpHandler for EchoHandler {
        fn handle(&self, action_json: &str, correlation_id: &str) -> serde_json::Value {
            serde_json::json!({
                "ok": true,
                "echoed_action": action_json,
                "correlation_id": correlation_id,
            })
        }
    }

    #[test]
    fn handler_can_be_stored_in_slot_and_invoked() {
        let slot = new_mls_op_handler_slot();
        // Empty slot is the default — nothing to invoke.
        assert!(slot.lock().unwrap().is_none());

        // Install the handler (the pattern `NmpApp::set_mls_op_handler` uses).
        *slot.lock().unwrap() = Some(Arc::new(EchoHandler) as Arc<dyn MlsOpHandler>);

        // The actor's `DispatchMlsOp` arm pulls the handler out under the
        // lock (cloning the inner `Arc`) and calls `handle` WITHOUT holding
        // the outer mutex — proven here by dropping the guard before the call.
        let cloned = {
            let guard = slot.lock().unwrap();
            guard.as_ref().cloned()
        };
        let handler = cloned.expect("handler should have been installed");
        let result = handler.handle(r#"{"op":"ping"}"#, "corr-test");
        assert_eq!(
            result.get("ok").and_then(|v| v.as_bool()),
            Some(true),
        );
        assert_eq!(
            result.get("correlation_id").and_then(|v| v.as_str()),
            Some("corr-test"),
        );
    }

    #[test]
    fn second_set_replaces_first_handler() {
        // Two distinct handlers with different identifying responses; the
        // second `set` MUST replace the first so the host can hot-swap (e.g.
        // on account switch).
        struct A;
        impl MlsOpHandler for A {
            fn handle(&self, _: &str, _: &str) -> serde_json::Value {
                serde_json::json!({"who": "A"})
            }
        }
        struct B;
        impl MlsOpHandler for B {
            fn handle(&self, _: &str, _: &str) -> serde_json::Value {
                serde_json::json!({"who": "B"})
            }
        }
        let slot = new_mls_op_handler_slot();
        *slot.lock().unwrap() = Some(Arc::new(A) as Arc<dyn MlsOpHandler>);
        *slot.lock().unwrap() = Some(Arc::new(B) as Arc<dyn MlsOpHandler>);
        let handler = slot.lock().unwrap().as_ref().cloned().unwrap();
        let result = handler.handle("{}", "x");
        assert_eq!(result.get("who").and_then(|v| v.as_str()), Some("B"));
    }
}
