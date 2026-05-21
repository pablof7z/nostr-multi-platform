//! `ActionRegistry` — the runtime that drives the `ActionModule` trait.
//!
//! # What this is (and is NOT)
//!
//! `substrate::ActionModule` has 15+ implementations (`PublishModule`, the
//! NIP-29 actions, and other app-module actions). This module is the dispatch
//! table that drives into them.
//!
//! This is deliberately NOT the deleted `ModuleRegistry` that
//! `substrate/mod.rs` warns about. That registry "only collected
//! `(namespace, family, type_name)` strings — nothing ever read them back."
//! This registry stores live `dyn ErasedActionModule` trait objects and
//! [`ActionRegistry::start`] actually *invokes* `ActionModule::start`. The
//! read-back path is real: [`crate::ffi`]'s `nmp_app_dispatch_action` calls
//! [`ActionRegistry::start`] and returns the resulting correlation id.
//!
//! # Scope (validation + execution, both in the registry)
//!
//! This registry performs **action validation, correlation-id assignment,
//! AND execution dispatch**:
//!
//! * [`ActionRegistry::start`] validates and assigns a correlation id.
//! * [`ActionRegistry::execute`] drives the validated action to the actor.
//!   Each module registers an executor closure via
//!   [`ActionRegistry::register_executor`]; `ffi::action::execute_action`
//!   is now a one-liner that calls `execute`. This eliminates the hardcoded
//!   `match namespace { "nmp.publish" => … }` that prevented any
//!   module from running without editing `nmp-core`.
//!
//! # Type erasure
//!
//! `ActionModule` is generic over an associated `Action` type, so a `HashMap`
//! of trait objects needs a dyn-safe facade. [`ErasedActionModule`] is that
//! facade: it speaks `serde_json::Value` at the boundary and
//! [`ActionModuleAdapter`] translates to/from each module's concrete
//! `Action` type via serde.

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::substrate::{
    ActionContext, ActionId, ActionModule, ActionRejection, ActionResult,
};

/// Dyn-safe facade over [`ActionModule`].
///
/// `ActionModule` carries an associated `Action` type, so it cannot be stored
/// as `Box<dyn ActionModule>` directly. This trait erases it to a JSON string
/// at the boundary so the registry can hold a heterogeneous map of modules.
/// [`ActionModuleAdapter`] is the only implementor; it round-trips each
/// module's typed action shape through serde.
trait ErasedActionModule: Send + Sync {
    /// Validate `action_json` against the module's `Action` type and return
    /// an optional preferred correlation id. Mirrors [`ActionModule::start`] +
    /// [`ActionModule::preferred_action_id`].
    ///
    /// `None` preferred id → caller uses [`new_action_id`]. `Some(id)` →
    /// caller uses that id directly (e.g. the signed event's `id` field for
    /// `PublishAction::Publish`, so that `dispatch_action`'s return and the
    /// matching `action_results` entry share the same identifier).
    fn start(
        &self,
        ctx: &mut ActionContext,
        action_json: &str,
    ) -> Result<Option<ActionId>, ActionRejection>;

    /// Whether this module carries a typed executor in `execute()`.
    ///
    /// `ActionModuleAdapter<M>` returns `true` — `execute()` calls
    /// `M::execute` directly. `ClosureModule` returns `false` — the caller
    /// must fall back to the `executors` HashMap for execution (the pre-
    /// ADR-0027 path kept for compatibility during migration).
    fn has_typed_executor(&self) -> bool;

    /// Execute the validated action. Called by [`ActionRegistry::execute`]
    /// only when [`Self::has_typed_executor`] returns `true`.
    fn execute(
        &self,
        action_json: &str,
        correlation_id: &str,
        send: &dyn Fn(crate::actor::ActorCommand),
    ) -> Result<(), String>;
}

/// Zero-sized adapter binding a concrete [`ActionModule`] `M` to the
/// dyn-safe [`ErasedActionModule`] facade. Holds no state — every method is
/// a static call into `M` with serde translation on the boundary.
struct ActionModuleAdapter<M: ActionModule>(std::marker::PhantomData<M>);

impl<M: ActionModule> Default for ActionModuleAdapter<M> {
    fn default() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<M: ActionModule> ErasedActionModule for ActionModuleAdapter<M> {
    fn start(
        &self,
        ctx: &mut ActionContext,
        action_json: &str,
    ) -> Result<Option<ActionId>, ActionRejection> {
        let action: M::Action = serde_json::from_str(action_json)
            .map_err(|e| ActionRejection::Invalid(e.to_string()))?;
        // Query preferred id before moving `action` into `M::start`.
        let preferred_id = M::preferred_action_id(&action);
        M::start(ctx, action)?;
        Ok(preferred_id)
    }

    fn has_typed_executor(&self) -> bool {
        true
    }

    fn execute(
        &self,
        action_json: &str,
        correlation_id: &str,
        send: &dyn Fn(crate::actor::ActorCommand),
    ) -> Result<(), String> {
        let action: M::Action = serde_json::from_str(action_json)
            .map_err(|e| e.to_string())?;
        M::execute(action, correlation_id, send)
    }
}

/// Dyn-safe executor closure type. Receives the already-validated action
/// JSON, the registry-minted `correlation_id`, and a `send` callback that
/// routes an [`ActorCommand`] to the actor. Returns `Ok(())` when the actor
/// command was queued, `Err(msg)` on decode or dispatch failure.
///
/// The `correlation_id` is the handle [`ActionRegistry::start`] minted and
/// the host received from `nmp_app_dispatch_action`. An executor that builds
/// an `ActorCommand` whose eventual terminal verdict must match that handle
/// (e.g. `nmp.publish`'s `PublishNote` — the actor signs the event, so its
/// `id` is unknown at dispatch time) threads this id onto the command so the
/// publish engine reports it in `action_results` instead of the signed
/// event's id.
type ExecutorFn = Box<
    dyn Fn(&str, &str, &dyn Fn(crate::actor::ActorCommand)) -> Result<(), String> + Send + Sync,
>;

/// Dyn-safe host-validator closure type — the [`ErasedActionModule::start`]
/// boundary minus the unused [`ActionContext`] (a host validator works from
/// the action JSON alone). `Ok(())` accepts the action, `Err` rejects it.
type ValidatorFn =
    Box<dyn Fn(&str) -> Result<(), ActionRejection> + Send + Sync>;

/// Shared, mutable slot holding the optional host-registered action-result
/// observer.
///
/// `Arc<Mutex<…>>` so [`ActionRegistry::set_result_observer`] and
/// [`ActionRegistry::deliver_result`] both take `&self` — registration and
/// delivery never need `&mut ActionRegistry`. The observer fires from the FFI
/// dispatch thread (where the registry already lives), so this slot does NOT
/// cross the actor/kernel boundary; it stays a private detail of the registry.
pub(crate) type ResultObserverSlot =
    Arc<Mutex<Option<Box<dyn Fn(ActionResult) + Send + Sync + 'static>>>>;

/// [`ErasedActionModule`] implementor backed by a host-supplied validator
/// closure rather than a compile-time [`ActionModule`] type — the *module*
/// counterpart to the host-registered *executor*. It wires the `start()`
/// validation half of a namespace into the registry *without editing
/// `nmp-core`*, slotting into the same `modules` map as [`ActionModuleAdapter`].
struct ClosureModule {
    validate: ValidatorFn,
}

impl ErasedActionModule for ClosureModule {
    /// Delegate validation to the host closure. `ctx` is unused — a host
    /// validator works from the action JSON alone (the typed
    /// [`ActionModuleAdapter`] path is the one that threads `ctx` into
    /// `ActionModule::start`).
    ///
    /// D6: the host validator is untrusted plugin code registered via
    /// `nmp_app_register_action_module`, and this runs on the call path of
    /// `nmp_app_dispatch_action` — an `extern "C"` function. An unguarded
    /// panic would unwind across the FFI boundary, which is undefined
    /// behaviour. The closure is therefore invoked inside [`catch_unwind`]; a
    /// caught panic becomes a plain [`ActionRejection::Invalid`], exactly the
    /// rejection a host would see for any other malformed action.
    fn start(
        &self,
        _ctx: &mut ActionContext,
        action_json: &str,
    ) -> Result<Option<ActionId>, ActionRejection> {
        // `AssertUnwindSafe`: a boxed `Fn` closure is not `UnwindSafe`, but a
        // panic here is fully contained — nothing the closure touched is
        // observed again after it unwinds, so there is no broken-invariant
        // hazard. `catch_unwind` nests the inner `Result`; flatten it.
        // Host-supplied validators have no natural correlation id to suggest,
        // so the preferred id is always `None`.
        match catch_unwind(AssertUnwindSafe(|| (self.validate)(action_json))) {
            Ok(result) => result.map(|()| None),
            Err(_) => Err(ActionRejection::Invalid(
                "action validator panicked".into(),
            )),
        }
    }

    fn has_typed_executor(&self) -> bool {
        false
    }

    fn execute(
        &self,
        _action_json: &str,
        _correlation_id: &str,
        _send: &dyn Fn(crate::actor::ActorCommand),
    ) -> Result<(), String> {
        // ClosureModule has no typed executor — ActionRegistry::execute falls
        // through to the executors HashMap for this module type.
        Err("ClosureModule: use executors fallback".into())
    }
}

/// Namespace-keyed registry of [`ActionModule`]s.
///
/// Stateless apart from the module and executor tables: every registered
/// module is a ZST adapter (cheap, `Send + Sync`). [`Self::start`] validates
/// and assigns a correlation id; [`Self::execute`] drives the validated
/// action to the actor. A module with no registered executor returns
/// `Err("no executor registered for namespace '…'")` from `execute` — the
/// caller surfaces this as `{"error":…}` (D6).
pub struct ActionRegistry {
    modules: HashMap<String, Box<dyn ErasedActionModule>>,
    executors: HashMap<String, ExecutorFn>,
    /// Optional host-registered observer notified when an action is accepted
    /// and enqueued. See [`Self::set_result_observer`] /
    /// [`Self::deliver_result`]. `None` until a host registers one — an
    /// unregistered observer makes delivery a silent no-op.
    result_observer: ResultObserverSlot,
}

impl Default for ActionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionRegistry {
    /// An empty registry. Call [`Self::register`] and
    /// [`Self::register_executor`] for each module.
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
            executors: HashMap::new(),
            result_observer: Arc::new(Mutex::new(None)),
        }
    }

    /// Register module `M` under its [`ActionModule::NAMESPACE`]. A second
    /// registration of the same namespace replaces the first.
    pub fn register<M: ActionModule + 'static>(&mut self) {
        self.modules.insert(
            M::NAMESPACE.to_string(),
            Box::new(ActionModuleAdapter::<M>::default()),
        );
    }

    /// Register an executor closure for `namespace`. The closure receives the
    /// validated action JSON, the registry-minted `correlation_id`, and a
    /// `send` callback; it converts the action to an [`ActorCommand`] and calls
    /// `send(cmd)`. A second registration replaces the first.
    pub fn register_executor(
        &mut self,
        namespace: impl Into<String>,
        f: impl Fn(&str, &str, &dyn Fn(crate::actor::ActorCommand)) -> Result<(), String>
            + Send
            + Sync
            + 'static,
    ) {
        self.executors.insert(namespace.into(), Box::new(f));
    }

    /// Register a host-provided closure as the *module validator* for
    /// `namespace`. The closure receives the raw action JSON and returns
    /// `Ok(())` to accept it or an [`ActionRejection`] to reject it.
    ///
    /// This is the complement to [`Self::register_executor`]: that wires the
    /// `execute()` half of a namespace, this wires the `start()` validation
    /// half. Together they let a host make a custom namespace fully reachable
    /// through `nmp_app_dispatch_action` — `start()` validates against this
    /// closure, `execute()` runs the registered executor — without adding an
    /// [`ActionModule`] type or editing [`default_registry`]. A second
    /// registration of the same namespace replaces the first.
    pub fn register_with_validator(
        &mut self,
        namespace: impl Into<String>,
        validate: impl Fn(&str) -> Result<(), ActionRejection> + Send + Sync + 'static,
    ) {
        self.modules.insert(
            namespace.into(),
            Box::new(ClosureModule {
                validate: Box::new(validate),
            }),
        );
    }

    /// Validate `action_json` against the module registered under
    /// `namespace`, returning the action's correlation id.
    ///
    /// An unknown namespace is an [`ActionRejection::Invalid`]; a JSON shape
    /// that does not match the module's `Action` type is also
    /// `ActionRejection::Invalid` (surfaced from the adapter). The
    /// correlation id is generated *after* validation succeeds so a rejected
    /// action never consumes one.
    ///
    /// The returned id is either the module's [`ActionModule::preferred_action_id`]
    /// (when the module returns `Some`) or a freshly minted [`new_action_id`].
    /// Using the preferred id makes `dispatch_action`'s JSON return and the
    /// matching `action_results` entry use the same identifier — a requirement
    /// for hosts that key UI spinners on the returned `correlation_id`.
    pub fn start(
        &self,
        ctx: &mut ActionContext,
        namespace: &str,
        action_json: &str,
    ) -> Result<ActionId, ActionRejection> {
        let module = self.modules.get(namespace).ok_or_else(|| {
            ActionRejection::Invalid(format!("unknown action namespace: {namespace}"))
        })?;
        let preferred_id = module.start(ctx, action_json)?;
        Ok(preferred_id.unwrap_or_else(new_action_id))
    }

    /// Execute the validated action. Tries the typed [`ActionModule::execute`]
    /// path first (ADR-0027), then falls back to the closure-based `executors`
    /// HashMap for namespaces registered via [`Self::register_executor`].
    ///
    /// Returns `Err` when neither path has an executor — the caller surfaces
    /// this as `{"error":…}` (D6: a missing executor is never silently
    /// swallowed).
    ///
    /// D6: both paths invoke untrusted host-plugin code that runs on the
    /// `nmp_app_dispatch_action` call path (an `extern "C"` function). Each
    /// is wrapped in [`catch_unwind`]; a caught panic returns `Err(String)`
    /// rather than unwinding across the FFI boundary.
    pub(crate) fn execute(
        &self,
        namespace: &str,
        action_json: &str,
        correlation_id: &str,
        send: &dyn Fn(crate::actor::ActorCommand),
    ) -> Result<(), String> {
        // ADR-0027 typed path: if the module has a built-in executor, use it.
        if let Some(module) = self.modules.get(namespace) {
            if module.has_typed_executor() {
                return match catch_unwind(AssertUnwindSafe(|| {
                    module.execute(action_json, correlation_id, send)
                })) {
                    Ok(result) => result,
                    Err(_) => Err("action executor panicked".to_string()),
                };
            }
        }
        // Pre-ADR-0027 fallback: closure registered via register_executor.
        match self.executors.get(namespace) {
            Some(exec) => {
                match catch_unwind(AssertUnwindSafe(|| {
                    exec(action_json, correlation_id, send)
                })) {
                    Ok(result) => result,
                    Err(_) => Err("action executor panicked".to_string()),
                }
            }
            None => Err(format!(
                "no executor registered for namespace '{namespace}'"
            )),
        }
    }

    /// Register the host-supplied action-result observer.
    ///
    /// The observer is the *push* counterpart to the snapshot-projection
    /// (pull) output seam: after [`Self::execute`] returns `Ok` for a
    /// dispatched action, [`Self::deliver_result`] hands the observer an
    /// [`ActionResult`] carrying the action's `correlation_id`. This is an
    /// "action accepted and enqueued" signal — for `nmp.publish` the actor
    /// still has to verify+publish after this fires (see [`ActionResult`]).
    ///
    /// Takes `&self`: the observer lives behind an `Arc<Mutex<…>>` slot, so a
    /// host may register it before *or after* `nmp_app_start`. A second
    /// registration replaces the first. A poisoned slot is a silent no-op
    /// (D6 — a bad registration never crashes the host).
    pub fn set_result_observer(
        &self,
        f: impl Fn(ActionResult) + Send + Sync + 'static,
    ) {
        if let Ok(mut slot) = self.result_observer.lock() {
            *slot = Some(Box::new(f));
        }
    }

    /// Deliver `result` to the registered observer, if any.
    ///
    /// A no-op when no observer is registered, or when the observer slot
    /// mutex is poisoned (D6 — delivery failures are never a crash). Holding
    /// the lock across the observer call is intentional: registration is a
    /// host-init-time event, so contention with [`Self::set_result_observer`]
    /// is not expected.
    ///
    /// D6: the observer is untrusted host plugin code registered via
    /// `nmp_app_register_action_result_observer`, and this runs on the call
    /// path of `nmp_app_dispatch_action` — an `extern "C"` function. An
    /// unguarded panic would (a) poison the slot mutex, silently disabling
    /// all future delivery, and (b) unwind across the FFI boundary
    /// (undefined behaviour). The observer is therefore invoked inside
    /// [`catch_unwind`]: a caught panic drops this result and leaves the
    /// observer registered so the next `deliver_result` still fires, exactly
    /// matching the per-callback panic-isolation pattern used by the actor
    /// loop's relay-event observer (`actor/mod.rs`).
    ///
    /// `AssertUnwindSafe`: a boxed `Fn` closure is not `UnwindSafe`, but a
    /// panic here is fully contained — nothing the closure touched is
    /// observed again after it unwinds (this `&self` method holds no
    /// invariants past the call), so there is no broken-invariant hazard.
    pub fn deliver_result(&self, result: ActionResult) {
        if let Ok(slot) = self.result_observer.lock() {
            if let Some(observer) = slot.as_ref() {
                // The panic is swallowed: this `ActionResult` is dropped and
                // future deliveries still fire. The default panic hook still
                // prints the payload, so the bug stays visible to ops.
                let _ = catch_unwind(AssertUnwindSafe(|| observer(result)));
            }
        }
    }

    /// `true` when a module is registered under `namespace`.
    #[cfg(test)]
    pub fn contains(&self, namespace: &str) -> bool {
        self.modules.contains_key(namespace)
    }
}

/// Generate a unique 32-hex-char action correlation id.
///
/// Combines a wall-clock nanosecond stamp with a process-lifetime atomic
/// counter so two ids minted in the same nanosecond still differ. This is a
/// correlation handle, not a security token — no cryptographic randomness is
/// required (the M6 ledger may swap in a UUID later without touching
/// callers).
fn new_action_id() -> ActionId {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    // 96-bit nanos truncated to the low 64 bits + a 64-bit sequence → 32 hex.
    format!("{:016x}{:016x}", nanos as u64, seq)
}

/// Build the registry the kernel ships with.
///
/// Only [`crate::publish::PublishModule`] is registered here. NIP-29 group
/// actions and the NIP-59 welcome-wrap module are *app* nouns (D0 —
/// `nmp-core` never names a protocol crate); the app host registers those
/// against its own registry instance via [`ActionRegistry::register`] +
/// [`ActionRegistry::register_executor`].
pub fn default_registry() -> ActionRegistry {
    use crate::actor::ActorCommand;
    use crate::publish::PublishAction;

    let mut registry = ActionRegistry::new();
    registry.register::<crate::publish::PublishModule>();
    registry.register_executor("nmp.publish", |action_json, correlation_id, send| {
        let action: PublishAction = serde_json::from_str(action_json)
            .map_err(|e| format!("publish action decode failed: {e}"))?;
        match action {
            PublishAction::Publish { event, target, .. } => {
                // D8 — non-blocking channel send only; the actor loop
                // owns signing/publishing (D4). The event is already
                // signed; the actor re-verifies it before publishing.
                //
                // Thread the registry-minted `correlation_id` onto the
                // command (PR-A — explicit symmetry with `PublishNote`).
                // For pre-signed `Publish`, `PublishModule::preferred_action_id()`
                // returns the event's `id`, so the minted id passed here is
                // == `event.id` == the engine's `PublishHandle`; threading
                // it makes the publish engine's `correlation_id_override`
                // do the same work explicitly instead of falling back to
                // the handle by coincidence. Defends the round-trip against
                // future changes that would decouple the dispatch return
                // value from the publish handle.
                send(ActorCommand::PublishSignedEvent {
                    raw: signed_event_to_raw(event),
                    relays: relays_for_target(&target),
                    correlation_id: Some(correlation_id.to_string()),
                });
                Ok(())
            }
            // D8 — non-blocking channel send only; the actor loop signs
            // the kind:1 with the active account (D4). This is the
            // `ActionModule`-native replacement for the deleted per-verb
            // `nmp_app_publish_note` FFI symbol — same `ActorCommand`,
            // same runtime path.
            //
            // The event id is NOT known at dispatch time (the actor signs
            // it), so `preferred_action_id()` returns `None` and the
            // registry minted a random `correlation_id`. Thread that id
            // onto the command so the publish engine reports it in
            // `action_results` instead of the signed event's `id` —
            // otherwise the host's spinner (keyed on the dispatch return
            // value) could never be cleared.
            PublishAction::PublishNote { content, reply_to_id, .. } => {
                send(ActorCommand::PublishNote {
                    content,
                    reply_to_id,
                    correlation_id: Some(correlation_id.to_string()),
                });
                Ok(())
            }
            // D8 — non-blocking channel send only; the actor loop builds the
            // kind:0 event, stamps `created_at`, and signs with the active
            // account (D4/D7). The `ActionModule`-native path for kind:0
            // metadata publish; PR-F deleted the prior
            // `nmp_app_publish_unsigned_event` FFI symbol, so this is the
            // sole entrypoint for it.
            //
            // The event id is NOT known at dispatch time (the actor signs it),
            // so `preferred_action_id()` returns `None` and the registry minted
            // a random `correlation_id`. Thread that id onto the command so the
            // publish engine reports it in `action_results`.
            PublishAction::PublishProfile { fields } => {
                send(ActorCommand::PublishProfile {
                    fields,
                    correlation_id: Some(correlation_id.to_string()),
                });
                Ok(())
            }
            // Unreachable in practice: `PublishModule::start` rejects `Cancel`
            // before the registry ever runs this executor, so a `Cancel`
            // action never gets here. The arm exists only for match
            // exhaustiveness — D6 forbids `unreachable!()` on a production
            // path, hence a bare `Ok(())`. Publish cancel is driven by the
            // `nmp_app_cancel_publish` FFI symbol, not `dispatch_action`.
            PublishAction::Cancel { .. } => Ok(()),
        }
    });
    registry
}

/// Convert a [`SignedEvent`] (the publish-action / engine input shape) into
/// a flat NIP-01 [`crate::store::RawEvent`] (the actor command shape). Pure
/// field move — `id` and `sig` are carried verbatim, no re-signing. This is
/// the inverse of the `RawEvent → SignedEvent` conversion in
/// `actor::commands::publish::publish_signed_event`.
fn signed_event_to_raw(event: crate::substrate::SignedEvent) -> crate::store::RawEvent {
    crate::store::RawEvent {
        id: event.id,
        pubkey: event.unsigned.pubkey,
        created_at: event.unsigned.created_at,
        kind: event.unsigned.kind,
        tags: event.unsigned.tags,
        content: event.unsigned.content,
        sig: event.sig,
    }
}

/// Resolve a [`crate::publish::PublishTarget`] into the relay slice
/// [`crate::actor::ActorCommand::PublishSignedEvent`] expects: `Auto` →
/// empty (NIP-65 outbox resolver, D3 default), `Explicit` → the named
/// opt-out relays.
fn relays_for_target(target: &crate::publish::PublishTarget) -> Vec<crate::publish::RelayUrl> {
    match target {
        crate::publish::PublishTarget::Auto => Vec::new(),
        crate::publish::PublishTarget::Explicit { relays } => relays.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::substrate::{SignedEvent, UnsignedEvent};

    fn ctx() -> ActionContext {
        ActionContext { now_ms: 1_700_000_000_000 }
    }

    /// A `SignedEvent` with non-empty `id`/`sig` — enough to pass
    /// `PublishModule::start`'s "requires a signed event" gate. The content
    /// is irrelevant: `start` never inspects `unsigned`.
    fn fixture_signed_event() -> SignedEvent {
        SignedEvent {
            id: "a".repeat(64),
            sig: "b".repeat(128),
            unsigned: UnsignedEvent {
                pubkey: "c".repeat(64),
                kind: 1,
                tags: Vec::new(),
                content: "test".to_string(),
                created_at: 1_700_000_000,
            },
        }
    }

    #[test]
    fn default_registry_has_publish_module() {
        let registry = default_registry();
        assert!(registry.contains("nmp.publish"));
        assert!(!registry.contains("nmp.nope"));
    }

    #[test]
    fn start_publish_note_action_returns_correlation_id() {
        // `PublishAction::PublishNote` only needs non-empty content — it
        // exercises the full registry → adapter → module::start path
        // without needing a fully-signed event fixture. The actor signs the
        // note, so `preferred_action_id` returns `None` and the registry
        // mints a random 32-hex-char `correlation_id`.
        let registry = default_registry();
        let action_json =
            r#"{"PublishNote":{"content":"hello","reply_to_id":null,"target":"Auto"}}"#;
        let id = registry
            .start(&mut ctx(), "nmp.publish", action_json)
            .expect("publish note action should be accepted");
        assert_eq!(id.len(), 32, "correlation id should be 32 hex chars");
        assert!(
            id.chars().all(|c| c.is_ascii_hexdigit()),
            "correlation id should be hex: {id}"
        );
    }

    #[test]
    fn start_cancel_action_is_rejected_via_dispatch() {
        // Publish cancel is engine-internal — it is driven by the
        // `nmp_app_cancel_publish` FFI symbol, never `dispatch_action`.
        // `PublishModule::start` therefore rejects a `Cancel` action so the
        // generic action seam carries nothing for cancel.
        let registry = default_registry();
        let action_json = r#"{"Cancel":{"handle":"smoke-test"}}"#;
        let err = registry
            .start(&mut ctx(), "nmp.publish", action_json)
            .expect_err("cancel must not be dispatchable via dispatch_action");
        match err {
            ActionRejection::Invalid(msg) => {
                assert!(
                    msg.contains("nmp_app_cancel_publish"),
                    "rejection should point at the FFI symbol: {msg}"
                );
            }
            other => panic!("expected Invalid rejection, got {other:?}"),
        }
    }

    #[test]
    fn start_publish_action_with_signed_event_is_accepted() {
        // A `PublishAction::Publish` with a non-empty id+sig passes
        // `PublishModule::start`'s validation gate.
        //
        // `preferred_action_id` returns the event's `id` (64 hex chars) so that
        // `dispatch_action`'s return value and `action_results` in the
        // snapshot share the same identifier. The fixture event has `id =
        // "a".repeat(64)` — 64 hex chars, not the 32-char minted `new_action_id`.
        let registry = default_registry();
        let event = fixture_signed_event();
        let expected_id = event.id.clone();
        let action = crate::publish::PublishAction::Publish {
            handle: "h1".to_string(),
            event,
            target: crate::publish::PublishTarget::Auto,
        };
        let action_json = serde_json::to_string(&action).unwrap();
        let id = registry
            .start(&mut ctx(), "nmp.publish", &action_json)
            .expect("publish action with id+sig should be accepted");
        assert_eq!(id, expected_id, "Publish action must use event.id as correlation_id");
    }

    #[test]
    fn unknown_namespace_is_rejected() {
        let registry = default_registry();
        let err = registry
            .start(&mut ctx(), "nmp.does-not-exist", "{}")
            .expect_err("unknown namespace must be rejected");
        match err {
            ActionRejection::Invalid(msg) => {
                assert!(msg.contains("unknown action namespace"), "got: {msg}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn malformed_json_is_rejected_as_invalid() {
        let registry = default_registry();
        let err = registry
            .start(&mut ctx(), "nmp.publish", "{not valid json")
            .expect_err("malformed JSON must be rejected");
        assert!(
            matches!(err, ActionRejection::Invalid(_)),
            "expected Invalid, got {err:?}"
        );
    }

    #[test]
    fn json_not_matching_action_shape_is_rejected() {
        // Valid JSON, wrong shape for `PublishAction` — serde's externally
        // tagged enum expects `{"<Variant>": {...}}`, so a flat
        // `{"t":"PublishNote"}` matches no variant and is rejected.
        let registry = default_registry();
        let err = registry
            .start(&mut ctx(), "nmp.publish", r#"{"t":"PublishNote"}"#)
            .expect_err("wrong-shape JSON must be rejected");
        assert!(matches!(err, ActionRejection::Invalid(_)));
    }

    #[test]
    fn start_publish_note_action_with_content_is_accepted() {
        // `PublishAction::PublishNote` with non-empty content passes
        // `PublishModule::start`'s validation gate — the `ActionModule`-native
        // path replacing the deleted per-verb `nmp_app_publish_note` symbol.
        let registry = default_registry();
        let action_json =
            r#"{"PublishNote":{"content":"hello","reply_to_id":null,"target":"Auto"}}"#;
        let id = registry
            .start(&mut ctx(), "nmp.publish", action_json)
            .expect("publish-note action with content should be accepted");
        assert_eq!(id.len(), 32);
    }

    /// THE FIX: the `nmp.publish` executor threads the registry-minted
    /// `correlation_id` onto `ActorCommand::PublishNote`. The actor signs the
    /// event, so its id is unknown at dispatch time — without this, the
    /// publish engine would report the event id and the host's spinner (keyed
    /// on the dispatch return value) could never be cleared. This exercises
    /// the real `default_registry()` executor closure end-to-end via
    /// `execute()`, capturing the `ActorCommand` it sends.
    #[test]
    fn publish_note_executor_threads_correlation_id_onto_actor_command() {
        use crate::actor::ActorCommand;
        use std::sync::{Arc, Mutex};

        let registry = default_registry();
        let captured: Arc<Mutex<Option<ActorCommand>>> = Arc::new(Mutex::new(None));
        let captured_in_send = Arc::clone(&captured);

        let minted_correlation_id = "fe".repeat(16);
        let action_json =
            r#"{"PublishNote":{"content":"hello","reply_to_id":null,"target":"Auto"}}"#;
        registry
            .execute("nmp.publish", action_json, &minted_correlation_id, &|cmd| {
                *captured_in_send.lock().unwrap() = Some(cmd);
            })
            .expect("publish-note execution should succeed");

        let cmd = captured.lock().unwrap().take().expect("an ActorCommand must be sent");
        match cmd {
            ActorCommand::PublishNote {
                content,
                reply_to_id,
                correlation_id,
            } => {
                assert_eq!(content, "hello");
                assert_eq!(reply_to_id, None);
                assert_eq!(
                    correlation_id,
                    Some(minted_correlation_id),
                    "the executor must thread the minted correlation_id onto the command"
                );
            }
            other => panic!("expected ActorCommand::PublishNote, got {other:?}"),
        }
    }

    /// PR-A: the pre-signed `Publish` executor now threads the registry-minted
    /// `correlation_id` onto `ActorCommand::PublishSignedEvent` — explicit
    /// symmetry with the `PublishNote` path. The round-trip used to work by
    /// coincidence (`preferred_action_id` returns `event.id`, the engine's
    /// `None`-fallback also reports `event.id`); the explicit thread upgrades
    /// that coincidence into a guarantee the publish engine surfaces the
    /// dispatch-returned id even if future changes ever decouple the dispatch
    /// return value from the publish handle.
    #[test]
    fn publish_signed_executor_sends_publish_signed_event_command() {
        use crate::actor::ActorCommand;
        use std::sync::{Arc, Mutex};

        let registry = default_registry();
        let captured: Arc<Mutex<Option<ActorCommand>>> = Arc::new(Mutex::new(None));
        let captured_in_send = Arc::clone(&captured);

        let action = crate::publish::PublishAction::Publish {
            handle: "h-presigned".to_string(),
            event: fixture_signed_event(),
            target: crate::publish::PublishTarget::Auto,
        };
        let action_json = serde_json::to_string(&action).unwrap();
        let minted_correlation_id = "ae".repeat(16);
        registry
            .execute("nmp.publish", &action_json, &minted_correlation_id, &|cmd| {
                *captured_in_send.lock().unwrap() = Some(cmd);
            })
            .expect("publish execution should succeed");

        let cmd = captured.lock().unwrap().take().expect("an ActorCommand must be sent");
        match cmd {
            ActorCommand::PublishSignedEvent {
                correlation_id,
                ..
            } => {
                assert_eq!(
                    correlation_id,
                    Some(minted_correlation_id),
                    "the executor must thread the minted correlation_id onto the command"
                );
            }
            other => panic!(
                "a pre-signed Publish must route to PublishSignedEvent, got {other:?}"
            ),
        }
    }

    #[test]
    fn start_publish_profile_action_with_string_fields_is_accepted() {
        // `PublishAction::PublishProfile` with a flat string-valued `fields`
        // map passes `PublishModule::start`'s validation gate — the
        // `ActionModule`-native path for kind:0 metadata publish. PR-F
        // deleted the prior `nmp_app_publish_unsigned_event` FFI symbol;
        // this `nmp.publish` dispatch is the sole entrypoint for it.
        let registry = default_registry();
        let action_json =
            r#"{"PublishProfile":{"fields":{"name":"Alice","about":"hello"}}}"#;
        let id = registry
            .start(&mut ctx(), "nmp.publish", action_json)
            .expect("publish-profile action with string fields should be accepted");
        assert_eq!(id.len(), 32, "correlation id should be 32 hex chars");
        assert!(
            id.chars().all(|c| c.is_ascii_hexdigit()),
            "correlation id should be hex: {id}"
        );
    }

    #[test]
    fn start_publish_profile_action_with_non_string_field_is_rejected() {
        // A kind:0 `content` is a flat JSON object of string values — a
        // numeric (or any non-string) field is rejected at `start`.
        let registry = default_registry();
        let action_json = r#"{"PublishProfile":{"fields":{"name":"Alice","age":42}}}"#;
        let err = registry
            .start(&mut ctx(), "nmp.publish", action_json)
            .expect_err("non-string profile field must be rejected");
        match err {
            ActionRejection::Invalid(msg) => {
                assert!(
                    msg.contains("must be a string value"),
                    "got: {msg}"
                );
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    /// The `nmp.publish` executor threads the registry-minted `correlation_id`
    /// onto `ActorCommand::PublishProfile`. The actor signs the event, so its
    /// id is unknown at dispatch time — without this the publish engine could
    /// not report the host's correlation_id in `action_results`. Exercises
    /// the real `default_registry()` executor closure via `execute()`.
    #[test]
    fn publish_profile_executor_threads_correlation_id_onto_actor_command() {
        use crate::actor::ActorCommand;
        use std::sync::{Arc, Mutex};

        let registry = default_registry();
        let captured: Arc<Mutex<Option<ActorCommand>>> = Arc::new(Mutex::new(None));
        let captured_in_send = Arc::clone(&captured);

        let minted_correlation_id = "ab".repeat(16);
        let action_json =
            r#"{"PublishProfile":{"fields":{"name":"Alice","picture":"https://x/y.png"}}}"#;
        registry
            .execute("nmp.publish", action_json, &minted_correlation_id, &|cmd| {
                *captured_in_send.lock().unwrap() = Some(cmd);
            })
            .expect("publish-profile execution should succeed");

        let cmd = captured.lock().unwrap().take().expect("an ActorCommand must be sent");
        match cmd {
            ActorCommand::PublishProfile {
                fields,
                correlation_id,
            } => {
                assert_eq!(
                    fields.get("name").and_then(|v| v.as_str()),
                    Some("Alice"),
                    "the profile fields must be carried through verbatim"
                );
                assert_eq!(
                    fields.get("picture").and_then(|v| v.as_str()),
                    Some("https://x/y.png")
                );
                assert_eq!(
                    correlation_id,
                    Some(minted_correlation_id),
                    "the executor must thread the minted correlation_id onto the command"
                );
            }
            other => panic!("expected ActorCommand::PublishProfile, got {other:?}"),
        }
    }

    #[test]
    fn start_publish_note_action_with_empty_content_is_rejected() {
        // Empty content fails the `PublishModule::start` gate.
        let registry = default_registry();
        let action_json =
            r#"{"PublishNote":{"content":"","reply_to_id":null,"target":"Auto"}}"#;
        let err = registry
            .start(&mut ctx(), "nmp.publish", action_json)
            .expect_err("empty-content publish note must be rejected");
        match err {
            ActionRejection::Invalid(msg) => {
                assert!(msg.contains("non-empty content"), "got: {msg}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn deliver_result_invokes_registered_observer() {
        use std::sync::{Arc, Mutex};
        // The observer captures every `ActionResult` it receives.
        let seen: Arc<Mutex<Vec<ActionResult>>> = Arc::new(Mutex::new(Vec::new()));
        let seen_in_observer = Arc::clone(&seen);

        let registry = default_registry();
        registry.set_result_observer(move |result| {
            seen_in_observer.lock().unwrap().push(result);
        });

        registry.deliver_result(ActionResult {
            correlation_id: "abc123".to_string(),
            result_json: serde_json::Value::Null,
        });

        let captured = seen.lock().unwrap();
        assert_eq!(captured.len(), 1, "observer should be called exactly once");
        assert_eq!(
            captured[0].correlation_id, "abc123",
            "observer should receive the delivered correlation id"
        );
        assert!(
            captured[0].result_json.is_null(),
            "fire-and-forget delivery carries a null result_json"
        );
    }

    #[test]
    fn deliver_result_without_observer_is_silent_noop() {
        // No observer registered — delivery must not panic.
        let registry = default_registry();
        registry.deliver_result(ActionResult {
            correlation_id: "no-observer".to_string(),
            result_json: serde_json::Value::Null,
        });
    }

    #[test]
    fn set_result_observer_second_registration_replaces_first() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;
        let first = Arc::new(AtomicU32::new(0));
        let second = Arc::new(AtomicU32::new(0));
        let first_c = Arc::clone(&first);
        let second_c = Arc::clone(&second);

        let registry = default_registry();
        registry.set_result_observer(move |_| {
            first_c.fetch_add(1, Ordering::SeqCst);
        });
        registry.set_result_observer(move |_| {
            second_c.fetch_add(1, Ordering::SeqCst);
        });

        registry.deliver_result(ActionResult {
            correlation_id: "x".to_string(),
            result_json: serde_json::Value::Null,
        });

        assert_eq!(first.load(Ordering::SeqCst), 0, "first observer is replaced");
        assert_eq!(second.load(Ordering::SeqCst), 1, "second observer receives it");
    }

    #[test]
    fn correlation_ids_are_unique_across_calls() {
        let registry = default_registry();
        let action_json =
            r#"{"PublishNote":{"content":"x","reply_to_id":null,"target":"Auto"}}"#;
        let mut seen = std::collections::HashSet::new();
        for _ in 0..256 {
            let id = registry
                .start(&mut ctx(), "nmp.publish", action_json)
                .unwrap();
            assert!(seen.insert(id.clone()), "duplicate correlation id: {id}");
        }
    }

    /// D6 — a host validator closure that panics is contained: `start`
    /// returns [`ActionRejection::Invalid`] instead of unwinding.
    ///
    /// The validator is untrusted host plugin code registered via
    /// `register_with_validator` (the `nmp_app_register_action_module` seam).
    /// `start` is reached from `nmp_app_dispatch_action`, an `extern "C"`
    /// function — an unguarded panic would unwind across the FFI boundary
    /// (undefined behaviour). Without the per-closure `catch_unwind` guard
    /// this test panics out of `start` rather than returning a rejection.
    #[test]
    fn panicking_validator_is_rejected_not_unwound() {
        let mut registry = ActionRegistry::new();
        registry.register_with_validator("host.boom", |_action_json| {
            panic!("buggy host validator");
        });
        let err = registry
            .start(&mut ctx(), "host.boom", "{}")
            .expect_err("a panicking validator must be rejected, not unwound");
        match err {
            ActionRejection::Invalid(msg) => {
                assert_eq!(msg, "action validator panicked", "got: {msg}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    /// D6 — a host executor closure that panics is contained: `execute`
    /// returns `Err` instead of unwinding.
    ///
    /// The executor is untrusted host plugin code registered via
    /// `register_executor` (the `nmp_app_register_action_executor` seam).
    /// `execute` is reached from `nmp_app_dispatch_action`, an `extern "C"`
    /// function — an unguarded panic would unwind across the FFI boundary
    /// (undefined behaviour). Without the per-closure `catch_unwind` guard
    /// this test panics out of `execute` rather than returning `Err`.
    #[test]
    fn panicking_executor_returns_err_not_unwound() {
        let mut registry = ActionRegistry::new();
        registry.register_executor("host.boom", |_action_json, _correlation_id, _send| {
            panic!("buggy host executor");
        });
        let err = registry
            .execute("host.boom", "{}", "corr-id", &|_cmd| {})
            .expect_err("a panicking executor must return Err, not unwind");
        assert_eq!(err, "action executor panicked", "got: {err}");
    }

    /// D6 — a host result-observer closure that panics is contained:
    /// `deliver_result` swallows the unwind and the observer stays
    /// registered so the next result is still delivered.
    ///
    /// The observer is untrusted host plugin code registered via
    /// `set_result_observer` (the `nmp_app_register_action_result_observer`
    /// seam). `deliver_result` runs on the FFI dispatch thread — an
    /// unguarded panic would (a) poison the slot mutex (silently disabling
    /// all future delivery) and (b) unwind across the FFI boundary
    /// (undefined behaviour). The `catch_unwind` guard converts the panic
    /// into a per-result drop while leaving the observer live.
    #[test]
    fn panicking_result_observer_does_not_kill_delivery() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;

        let calls = Arc::new(AtomicU32::new(0));
        let calls_in_observer = Arc::clone(&calls);

        let registry = default_registry();
        registry.set_result_observer(move |result| {
            let n = calls_in_observer.fetch_add(1, Ordering::SeqCst) + 1;
            // Panic on the first call only — subsequent deliveries must
            // still reach the observer, proving panic isolation per-result.
            if n == 1 {
                panic!("buggy host result observer (call #{}, corr={})", n, result.correlation_id);
            }
        });

        // First delivery: observer panics, `deliver_result` must NOT
        // propagate it (this test would abort the process if it did).
        registry.deliver_result(ActionResult {
            correlation_id: "first".to_string(),
            result_json: serde_json::Value::Null,
        });
        // Second delivery: observer is still live and receives the call.
        registry.deliver_result(ActionResult {
            correlation_id: "second".to_string(),
            result_json: serde_json::Value::Null,
        });

        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "observer must have been invoked twice — once panicking, once successfully"
        );
    }
}
