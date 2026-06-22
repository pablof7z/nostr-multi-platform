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

    /// Byte-route (ADR-0064 / S3 + #1756 / S9) twin of [`Self::start`]: resolve
    /// the OPAQUE envelope `payload` into the module's `Action` and validate it.
    ///
    /// Two opt-in routes (see [`decode_byte_action`]): the TYPED FlatBuffers
    /// route ([`ActionModule::decode_payload`], with its fail-closed
    /// `schema_version` gate BEFORE `start()`), and the OPAQUE-PASSTHROUGH route
    /// for app-owned host-op modules ([`ActionModule::accepts_opaque_payload`]).
    /// Returns:
    /// * `Err(NotTypedCapable)` — the module opted into NEITHER route (untyped
    ///   AND not opaque-opted). Fail-closed: `start()` never ran.
    /// * `Err(Decode(_))` — typed route: a `schema_version` trip or malformed
    ///   FlatBuffers (fail-closed; `start()` never ran).
    /// * `Err(OpaqueMalformed(_))` — opaque route: the app's JSON-bytes payload
    ///   did not deserialize (fail-closed; `start()` never ran).
    /// * `Err(Rejected(_))` — the action decoded but `start()` rejected it.
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
    /// The namespace's module supports NEITHER byte route: it returns `None`
    /// from [`ActionModule::decode_payload`] (not typed) AND `false` from
    /// [`ActionModule::accepts_opaque_payload`] (not opaque-opted). The byte
    /// doorway fails closed on it rather than guessing — a non-opted untyped
    /// module never reaches `start()` (#1756 fail-closed).
    NotTypedCapable,
    /// The module opted into the opaque-passthrough route
    /// ([`ActionModule::accepts_opaque_payload`] is `true`) but the opaque
    /// payload bytes did not deserialize into its `Action` (the app owns this
    /// JSON-bytes format; a malformed payload fails closed — `start()` never
    /// ran). Distinct from [`Self::Decode`], which is a typed-FlatBuffers trip.
    OpaqueMalformed(String),
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
            Self::OpaqueMalformed(m) => write!(f, "opaque payload malformed: {m}"),
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
        // THE single byte-route decode site (ADR-0064 / S3 + #1756 / S9).
        // `decode_payload` is the typed opt-in (its `schema_version` gate runs
        // HERE, BEFORE `start()`); `accepts_opaque_payload` is the app-owned
        // opaque opt-in. Resolve typed-first, then opaque; a module that opted
        // into NEITHER fails closed (`NotTypedCapable`).
        let action = decode_byte_action::<M>(payload)?;
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
        let action = decode_byte_action::<M>(payload)?;
        self.0
            .execute(action, correlation_id, send)
            .map_err(TypedDispatchError::Execute)
    }
}

/// Resolve the OPAQUE envelope `payload` bytes into a module `M`'s `Action`,
/// applying the byte-route opt-in precedence (#1756 / S9 + ADR-0064 / S3). This
/// is the SOLE byte-route decode function — both `start_bytes` and
/// `execute_bytes` call it, mirroring how the JSON twins each
/// `serde_json::from_str` (one decoder, re-supplied per lifecycle phase, not two
/// divergent paths).
///
/// Precedence + fail-closed (#1756):
/// * `decode_payload` returns `Some` → TYPED FlatBuffers route (its fail-closed
///   `schema_version` gate ran inside it). This wins if a module opted into both.
/// * else `accepts_opaque_payload()` is `true` → OPAQUE-PASSTHROUGH route: the
///   bytes are the app's own JSON-bytes action; `serde_json::from_slice` is the
///   app's deserialize. NMP imposes no `schema_version` gate (the payload is
///   opaque to NMP); a malformed buffer fails closed as `OpaqueMalformed`.
/// * else → `NotTypedCapable`: the module opted into NEITHER route. A non-opted
///   untyped module (and, upstream, an unknown namespace) is REJECTED — never a
///   blanket default-accept.
fn decode_byte_action<M: ActionModule>(
    payload: &[u8],
) -> Result<M::Action, TypedDispatchError> {
    if let Some(decoded) = M::decode_payload(payload) {
        return decoded.map_err(TypedDispatchError::Decode);
    }
    if M::accepts_opaque_payload() {
        return serde_json::from_slice::<M::Action>(payload)
            .map_err(|e| TypedDispatchError::OpaqueMalformed(e.to_string()));
    }
    Err(TypedDispatchError::NotTypedCapable)
}
