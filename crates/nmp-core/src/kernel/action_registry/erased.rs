//! Dyn-erasure layer for [`super::ActionRegistry`].
//!
//! [`ActionModule`] carries an associated `Action` type, so a `HashMap` of
//! trait objects needs a dyn-safe facade. [`ErasedActionModule`] is that facade
//! and [`ActionModuleAdapter`] is the sole implementor, decoding each module's
//! concrete `Action` at the boundary.
//!
//! # Two boundary encodings, ONE decode site each
//!
//! * **JSON** (`start` / `execute` taking `action_json: &str`) — the legacy
//!   `nmp_app_dispatch_action(namespace, action_json)` doorway every module
//!   still supports; ADR-0064 migrates it away per-crate.
//! * **Typed FlatBuffers bytes** (`start_bytes` / `execute_bytes` taking
//!   `payload: &[u8]`) — ADR-0064 / S3 (#1751). This adapter is the SOLE
//!   decoder of the open transport's opaque per-crate payload: there is exactly
//!   ONE decode function, [`ActionModule::decode_payload`] (which delegates to
//!   the module's [`crate::substrate::ActionPayload`] impl, running the
//!   fail-closed `schema_version` gate BEFORE `start()`). Like the JSON twin —
//!   where `start` and `execute` each `serde_json::from_str` because the
//!   registry's two lifecycle phases re-supply the wire bytes rather than
//!   carrying the decoded value between calls — `start_bytes` and
//!   `execute_bytes` each invoke that ONE `decode_payload`. This is one decoder
//!   called twice across the lifecycle, NOT two divergent decode paths. A
//!   module that has not migrated returns `None` from `decode_payload`, and the
//!   typed doorway rejects its namespace as not-yet-typed.
//!
//! Extracted from `action_registry.rs` to keep that orchestrator file under the
//! 500-LOC hand-authored ceiling (AGENTS.md / V-12).

use crate::substrate::{
    ActionContext, ActionModule, ActionPayloadDecodeError, ActionRejection,
};

/// Dyn-safe facade over [`ActionModule`].
///
/// `ActionModule` carries an associated `Action` type, so it cannot be stored
/// as `Box<dyn ActionModule>` directly. This trait erases it to a JSON string
/// at the boundary so the registry can hold a heterogeneous map of modules.
/// [`ActionModuleAdapter`] is the sole implementor (ADR-0027 deleted the
/// pre-existing `ClosureModule` half); it round-trips each module's typed
/// action shape through serde.
pub(super) trait ErasedActionModule: Send + Sync {
    /// Validate `action_json` against the module's `Action` type. Mirrors
    /// [`ActionModule::start`].
    ///
    /// On `Ok`, the caller mints the operation's `correlation_id` via
    /// [`super::new_action_id`]. The `correlation_id` is the operation's sole
    /// identity end-to-end — it is NEVER substituted with output data such as a
    /// pre-signed event's `id` (the event id is the operation's *result*, not
    /// its identity; conflating them broke host spinner matching on the
    /// pre-signed publish path).
    fn start(
        &self,
        ctx: &mut ActionContext,
        action_json: &str,
    ) -> Result<(), ActionRejection>;

    /// Execute the validated action. Called by [`super::ActionRegistry::execute`]
    /// after `start` returns `Ok`.
    fn execute(
        &self,
        action_json: &str,
        correlation_id: &str,
        send: &dyn Fn(crate::actor::ActorCommand),
    ) -> Result<(), String>;

    /// Typed-payload (ADR-0064 / S3) twin of [`Self::start`]: decode the OPAQUE
    /// FlatBuffers `payload` into the module's `Action` and validate it.
    ///
    /// The decode runs the fail-closed `schema_version` gate BEFORE `start()`
    /// (inside [`ActionModule::decode_payload`] → the module's `ActionPayload`
    /// impl). Returns:
    /// * `Err(NotTypedCapable)` — the module has not migrated to a typed payload
    ///   (it left `decode_payload` defaulted). Distinct from a decode rejection.
    /// * `Err(Decode(_))` — a `schema_version` trip or a malformed buffer
    ///   (fail-closed; `start()` never ran).
    /// * `Err(Rejected(_))` — the typed action decoded but `start()` rejected it.
    /// * `Ok(())` — decoded and validated; the caller mints the `correlation_id`.
    fn start_bytes(
        &self,
        ctx: &mut ActionContext,
        payload: &[u8],
    ) -> Result<(), TypedDispatchError>;

    /// Typed-payload twin of [`Self::execute`]: decode `payload` and drive the
    /// validated action to the actor. Called after [`Self::start_bytes`] is
    /// `Ok`. A decode error here is a typed [`TypedDispatchError`] (it cannot be
    /// a `NotTypedCapable` if `start_bytes` already accepted, but the same shape
    /// keeps the seam uniform).
    fn execute_bytes(
        &self,
        payload: &[u8],
        correlation_id: &str,
        send: &dyn Fn(crate::actor::ActorCommand),
    ) -> Result<(), TypedDispatchError>;
}

/// Outcome of a typed-bytes (`*_bytes`) dispatch through the adapter. Errors are
/// **data** (D6) — none crosses the FFI/worker boundary as a panic/exception.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum TypedDispatchError {
    /// The namespace's module has not migrated to a typed FlatBuffers payload
    /// (it returns `None` from [`ActionModule::decode_payload`]). The typed
    /// doorway fails closed on it rather than guessing.
    NotTypedCapable,
    /// The typed payload failed to decode — a fail-closed `schema_version` trip
    /// or a malformed buffer. `start()` never ran.
    Decode(ActionPayloadDecodeError),
    /// The payload decoded but the module's `start()` validator rejected it.
    Rejected(ActionRejection),
    /// The module's `execute()` returned a synchronous error string.
    Execute(String),
}

impl core::fmt::Display for TypedDispatchError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotTypedCapable => {
                write!(f, "namespace does not support typed FlatBuffers payloads")
            }
            Self::Decode(e) => write!(f, "{e}"),
            Self::Rejected(r) => write!(f, "action rejected: {r:?}"),
            Self::Execute(m) => write!(f, "{m}"),
        }
    }
}

/// Adapter binding a concrete [`ActionModule`] `M` to the dyn-safe
/// [`ErasedActionModule`] facade.
///
/// ADR-0052 rung 5.2: the adapter holds the module **by value** (was a ZST
/// `PhantomData<M>`). This lets a stateful module own its dependencies (an
/// `Arc<WalletRuntimeHandle>`, an `Arc<DmRelayCache>`, …) captured by the host
/// at composition time, so `start`/`execute` reach that state through
/// `&self.0` rather than a process-global. Stateless modules are unit-shaped
/// values, so this stays effectively zero-cost for them.
pub(super) struct ActionModuleAdapter<M: ActionModule>(pub(super) M);

impl<M: ActionModule> ErasedActionModule for ActionModuleAdapter<M> {
    fn start(
        &self,
        ctx: &mut ActionContext,
        action_json: &str,
    ) -> Result<(), ActionRejection> {
        let action: M::Action = serde_json::from_str(action_json)
            .map_err(|e| ActionRejection::Invalid(e.to_string()))?;
        self.0.start(ctx, action)
    }

    fn execute(
        &self,
        action_json: &str,
        correlation_id: &str,
        send: &dyn Fn(crate::actor::ActorCommand),
    ) -> Result<(), String> {
        let action: M::Action = serde_json::from_str(action_json).map_err(|e| e.to_string())?;
        self.0.execute(action, correlation_id, send)
    }

    fn start_bytes(
        &self,
        ctx: &mut ActionContext,
        payload: &[u8],
    ) -> Result<(), TypedDispatchError> {
        // THE single typed-decode site (ADR-0064 / S3). `decode_payload` is the
        // module's opt-in hook; `None` means it has not migrated. The decode
        // (which runs the fail-closed `schema_version` gate) happens HERE,
        // BEFORE `start()`.
        let action = match M::decode_payload(payload) {
            None => return Err(TypedDispatchError::NotTypedCapable),
            Some(Err(e)) => return Err(TypedDispatchError::Decode(e)),
            Some(Ok(action)) => action,
        };
        self.0
            .start(ctx, action)
            .map_err(TypedDispatchError::Rejected)
    }

    fn execute_bytes(
        &self,
        payload: &[u8],
        correlation_id: &str,
        send: &dyn Fn(crate::actor::ActorCommand),
    ) -> Result<(), TypedDispatchError> {
        let action = match M::decode_payload(payload) {
            None => return Err(TypedDispatchError::NotTypedCapable),
            Some(Err(e)) => return Err(TypedDispatchError::Decode(e)),
            Some(Ok(action)) => action,
        };
        self.0
            .execute(action, correlation_id, send)
            .map_err(TypedDispatchError::Execute)
    }
}
