//! `HostOpHandler` — substrate-generic seam for stateful, host-owned op handlers
//! that the actor invokes from a typed `ActorCommand` arm.
//!
//! # Why this exists
//!
//! `ActionModule::execute` is a *static* method whose only output is enqueuing
//! `ActorCommand`s — by design, it has no access to per-app projection state.
//! `PublishModule`'s executor encodes everything it needs into a typed
//! `ActorCommand::PublishRawEvent { kind, tags, content, target, ... }` and the
//! actor's dispatch arm signs+publishes. That works because publish state lives
//! in the kernel.
//!
//! Some app crates own stateful runtime that the kernel cannot name (D0): the
//! Marmot MLS state (a per-process `MarmotService<MdkSqliteStorage>` holding
//! group ratchet secrets, processed Welcomes, key-package private keys) lives
//! in `nmp-app-marmot`, not `nmp-core`. The fixture crate's TODO-list
//! projection has the same shape — host-owned state that the kernel must not
//! name. For those crates an `ActorCommand` variant per op
//! (`ActorCommand::MarmotCreateGroup { ... }`, `ActorCommand::TodoAdd { ... }`)
//! would force `nmp-core` to name the app's nouns — exactly what D0 forbids.
//!
//! `HostOpHandler` is the boundary-shaped seam: a small, substrate-generic
//! trait `nmp-core` defines so the actor can ask "whoever owns the app-side
//! state, run this op for me" without knowing what the op is. The host
//! installs an `Arc<dyn HostOpHandler>` into [`NmpApp::set_host_op_handler`];
//! the [`crate::substrate::HostOpCommand`] (dispatched through the single
//! `ActorCommand::Protocol` write seam — ADR-0052 §D4, K2 rung 5.4) clones the
//! handler out of this slot at `run` time and calls [`HostOpHandler::handle`].
//! Before rung 5.4 a bespoke `ActorCommand::DispatchHostOp` arm did this; that
//! second write seam was deleted and merged into `Protocol`.
//!
//! # Naming (D0)
//!
//! The trait is named after its *role* in the substrate (`HostOpHandler` —
//! the host-installed handler for host-owned ops), NOT the protocol any one
//! consumer happens to implement. The kernel never grows app nouns: a name
//! like `MlsOpHandler` would have been wrong because it bakes one consumer's
//! protocol (MLS / Marmot) into the kernel's vocabulary, even when other
//! consumers (the fixture TODO-list crate, any future stateful app) use the
//! same seam with no MLS involvement. The legacy name was a D0 violation
//! corrected by this rename; the seam itself is unchanged.
//!
//! # The contract
//!
//! `handle` consumes a JSON op envelope (the action body's `Action` payload,
//! re-serialized to a string by the [`ActionModule::execute`] body) plus the
//! registry-minted `correlation_id`, runs the op synchronously on whatever
//! thread the `HostOpCommand` calls it from (the actor thread), and returns a
//! JSON value. The command wires that value into the `action_stages` /
//! `action_results` mirror keyed by `correlation_id` so the host can pick up
//! the result on the next tick (the same pull-model contract the snapshot
//! projection exposes).
//!
//! No protocol type ever crosses this boundary — the JSON string and
//! `serde_json::Value` are the only types it speaks. The handler impl on the
//! app side translates between its own typed action enum and this JSON
//! shape; `nmp-core` never sees the typed action enum.
//!
//! # D6 — no panic crosses the trait
//!
//! Implementations MUST NOT panic. The [`crate::substrate::HostOpCommand`]
//! wraps the call in `catch_unwind` (and the `Protocol` dispatch arm wraps the
//! whole command body too — ADR-0052 §D4), so a panic is converted to a
//! `Failed` action stage rather than unwinding the actor thread — but a
//! well-behaved impl returns an `{"ok":false,"error":...}` envelope for soft
//! failures instead of relying on the catch.
//!
//! # D8 — handlers must not block the actor thread for long
//!
//! `handle` runs *inline on the actor thread* (the same thread that drains
//! `ActorCommand`s and ticks the kernel). The current MLS-state consumer's
//! mutations are SQLite-bound and typically sub-100ms, which is within the
//! actor's tick budget. A handler whose op routinely exceeds ~50ms SHOULD
//! spawn a worker thread internally and fan a follow-up `ActorCommand` back
//! via the actor's self-feedback sender (the same pattern the NIP-57 LNURL
//! fetcher uses for its HTTP round-trip — see
//! `nmp_nip57::lnurl::FetchLnurlInvoiceCommand`, dispatched through
//! [`crate::actor::ActorCommand::Protocol`]). The trait does not enforce
//! this — it's the implementor's responsibility, same as for every other
//! `ActorCommand` dispatch arm.

use std::sync::{Arc, Mutex};

/// A host-installed handler for stateful, host-owned actions dispatched
/// through the [`crate::kernel::ActionRegistry`].
///
/// See the module rustdoc for the full contract. The blanket `Send + Sync`
/// bound is required because the handler is stored in a shared `Arc` slot
/// that the actor thread reads.
pub trait HostOpHandler: Send + Sync {
    /// Run one host-owned op.
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
    /// `{"ok":false,"error":...}` envelope by convention. The
    /// [`crate::substrate::HostOpCommand`] threads this value into the
    /// `action_stages` / `action_results` mirror keyed by `correlation_id`.
    ///
    /// MUST NOT panic (see D6 in the module docs).
    fn handle(&self, action_json: &str, correlation_id: &str) -> serde_json::Value;
}

/// Typed slot holding the host-installed [`HostOpHandler`].
///
/// `Arc<Mutex<Option<Arc<dyn HostOpHandler>>>>` because:
///
/// * the outer `Arc<Mutex<...>>` is the shared-slot pattern every other
///   `NmpApp` ↔ actor slot uses ([`crate::slots::MlsLocalNsecSlot`],
///   [`crate::slots::ActiveLocalKeysSlot`], etc.) — the `Mutex` is what
///   makes the slot writable without `&mut self` on `NmpApp`.
/// * the inner `Arc<dyn HostOpHandler>` is what the actor clones out under
///   the lock and calls — calling `handle` does NOT hold the outer mutex,
///   so a long-running handler does not block the FFI `set_host_op_handler`
///   write path.
pub type HostOpHandlerSlot = Arc<Mutex<Option<Arc<dyn HostOpHandler>>>>;

/// Construct a fresh, empty [`HostOpHandlerSlot`].
#[must_use]
pub fn new_host_op_handler_slot() -> HostOpHandlerSlot {
    Arc::new(Mutex::new(None))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trivial handler used to exercise the trait shape directly.
    struct EchoHandler;
    impl HostOpHandler for EchoHandler {
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
        let slot = new_host_op_handler_slot();
        // Empty slot is the default — nothing to invoke.
        assert!(slot.lock().unwrap().is_none());

        // Install the handler (the pattern `NmpApp::set_host_op_handler` uses).
        *slot.lock().unwrap() = Some(Arc::new(EchoHandler) as Arc<dyn HostOpHandler>);

        // The `HostOpHandlerAccessAdapter` pulls the handler out under the
        // lock (cloning the inner `Arc`) and calls `handle` WITHOUT holding
        // the outer mutex — proven here by dropping the guard before the call.
        let cloned = {
            let guard = slot.lock().unwrap();
            guard.as_ref().cloned()
        };
        let handler = cloned.expect("handler should have been installed");
        let result = handler.handle(r#"{"op":"ping"}"#, "corr-test");
        assert_eq!(result.get("ok").and_then(|v| v.as_bool()), Some(true),);
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
        impl HostOpHandler for A {
            fn handle(&self, _: &str, _: &str) -> serde_json::Value {
                serde_json::json!({"who": "A"})
            }
        }
        struct B;
        impl HostOpHandler for B {
            fn handle(&self, _: &str, _: &str) -> serde_json::Value {
                serde_json::json!({"who": "B"})
            }
        }
        let slot = new_host_op_handler_slot();
        *slot.lock().unwrap() = Some(Arc::new(A) as Arc<dyn HostOpHandler>);
        *slot.lock().unwrap() = Some(Arc::new(B) as Arc<dyn HostOpHandler>);
        let handler = slot.lock().unwrap().as_ref().cloned().unwrap();
        let result = handler.handle("{}", "x");
        assert_eq!(result.get("who").and_then(|v| v.as_str()), Some("B"));
    }
}
