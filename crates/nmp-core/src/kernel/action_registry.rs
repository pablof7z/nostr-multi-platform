//! `ActionRegistry` — the runtime that drives the `ActionModule` trait.
//!
//! # What this is (and is NOT)
//!
//! `substrate::ActionModule` has 16+ implementations (`PublishModule`, 15
//! NIP-29 actions, `WelcomeWrapModule`). This module is the dispatch table
//! that drives into them.
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
//! `ActionModule` is generic over associated types (`Action`, `Step`,
//! `Output`), so a `HashMap` of trait objects needs a dyn-safe facade.
//! [`ErasedActionModule`] is that facade: it speaks `serde_json::Value` at
//! the boundary and [`ActionModuleAdapter`] translates to/from each
//! module's concrete associated types via serde.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::substrate::{ActionContext, ActionId, ActionModule, ActionPlan, ActionRejection};

/// Dyn-safe facade over [`ActionModule`].
///
/// `ActionModule` carries three associated types, so it cannot be stored as
/// `Box<dyn ActionModule>` directly. This trait erases them to
/// `serde_json::Value` so the registry can hold a heterogeneous map of
/// modules. [`ActionModuleAdapter`] is the only implementor; it round-trips
/// each module's typed shapes through serde.
trait ErasedActionModule: Send + Sync {
    /// Validate `action_json` against the module's `Action` type and return
    /// the erased [`ActionPlan`]. Mirrors [`ActionModule::start`].
    fn start(
        &self,
        ctx: &mut ActionContext,
        action_json: &str,
    ) -> Result<ActionPlan<Value>, ActionRejection>;

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
    ) -> Result<ActionPlan<Value>, ActionRejection> {
        let action: M::Action = serde_json::from_str(action_json)
            .map_err(|e| ActionRejection::Invalid(e.to_string()))?;
        let plan = M::start(ctx, action)?;
        Ok(ActionPlan {
            initial_step: serde_json::to_value(&plan.initial_step).unwrap_or(Value::Null),
            initial_status: plan.initial_status,
            deadline_ms: plan.deadline_ms,
        })
    }

}

/// Dyn-safe executor closure type. Receives the already-validated action
/// JSON and a `send` callback that routes an [`ActorCommand`] to the actor.
/// Returns `Ok(())` when the actor command was queued, `Err(msg)` on decode
/// or dispatch failure.
type ExecutorFn =
    Box<dyn Fn(&str, &dyn Fn(crate::actor::ActorCommand)) -> Result<(), String> + Send + Sync>;

/// Namespace-keyed registry of [`ActionModule`]s.
///
/// Stateless apart from the module and executor tables: every registered
/// module is a ZST adapter (cheap, `Send + Sync`). [`Self::start`] validates
/// and assigns a correlation id; [`Self::execute`] drives the validated
/// action to the actor. A module with no registered executor returns
/// `Err("no executor registered for namespace '…'")` from `execute` — the
/// caller surfaces this as `{"error":…}` (D6).
pub struct ActionRegistry {
    modules: HashMap<&'static str, Box<dyn ErasedActionModule>>,
    executors: HashMap<&'static str, ExecutorFn>,
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
        }
    }

    /// Register module `M` under its [`ActionModule::NAMESPACE`]. A second
    /// registration of the same namespace replaces the first.
    pub fn register<M: ActionModule + 'static>(&mut self) {
        self.modules.insert(
            M::NAMESPACE,
            Box::new(ActionModuleAdapter::<M>::default()),
        );
    }

    /// Register an executor closure for `namespace`. The closure receives the
    /// validated action JSON and a `send` callback; it converts the action to
    /// an [`ActorCommand`] and calls `send(cmd)`. A second registration
    /// replaces the first.
    pub fn register_executor(
        &mut self,
        namespace: &'static str,
        f: impl Fn(&str, &dyn Fn(crate::actor::ActorCommand)) -> Result<(), String>
            + Send
            + Sync
            + 'static,
    ) {
        self.executors.insert(namespace, Box::new(f));
    }

    /// Validate `action_json` against the module registered under
    /// `namespace`, returning a fresh correlation id plus the erased
    /// [`ActionPlan`].
    ///
    /// An unknown namespace is an [`ActionRejection::Invalid`]; a JSON shape
    /// that does not match the module's `Action` type is also
    /// `ActionRejection::Invalid` (surfaced from the adapter). The
    /// correlation id is generated *after* validation succeeds so a rejected
    /// action never consumes one.
    pub fn start(
        &self,
        ctx: &mut ActionContext,
        namespace: &str,
        action_json: &str,
    ) -> Result<(ActionId, ActionPlan<Value>), ActionRejection> {
        let module = self.modules.get(namespace).ok_or_else(|| {
            ActionRejection::Invalid(format!("unknown action namespace: {namespace}"))
        })?;
        let plan = module.start(ctx, action_json)?;
        Ok((new_action_id(), plan))
    }

    /// Execute the validated action by invoking the registered executor for
    /// `namespace`. The `send` callback routes the resulting
    /// [`ActorCommand`] to the actor (D8: non-blocking channel send only).
    ///
    /// Returns `Err` when no executor is registered — the caller surfaces
    /// this as `{"error":…}` (D6: a missing executor is never silently
    /// swallowed). This eliminates the `match namespace { "nmp.publish" => …
    /// _ => Err }` that was the only execution path and blocked all other
    /// modules from running without editing `nmp-core`.
    pub(crate) fn execute(
        &self,
        namespace: &str,
        action_json: &str,
        send: &dyn Fn(crate::actor::ActorCommand),
    ) -> Result<(), String> {
        match self.executors.get(namespace) {
            Some(exec) => exec(action_json, send),
            None => Err(format!(
                "no executor registered for namespace '{namespace}'"
            )),
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
    registry.register_executor("nmp.publish", |action_json, send| {
        let action: PublishAction = serde_json::from_str(action_json)
            .map_err(|e| format!("publish action decode failed: {e}"))?;
        match action {
            PublishAction::Publish { event, target, .. } => {
                // D8 — non-blocking channel send only; the actor loop
                // owns signing/publishing (D4). The event is already
                // signed; the actor re-verifies it before publishing.
                send(ActorCommand::PublishSignedEvent {
                    raw: signed_event_to_raw(event),
                    relays: relays_for_target(&target),
                });
                Ok(())
            }
            // No publish-engine cancel command yet; the registry
            // already marked the action `Cancelled`.
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
    use crate::substrate::{ActionStatus, SignedEvent, UnsignedEvent};

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
    fn start_cancel_action_returns_correlation_id() {
        // `PublishAction::Cancel` only needs a non-empty handle — it
        // exercises the full registry → adapter → module::start path
        // without needing a fully-signed event fixture.
        let registry = default_registry();
        let action_json = r#"{"Cancel":{"handle":"smoke-test"}}"#;
        let (id, plan) = registry
            .start(&mut ctx(), "nmp.publish", action_json)
            .expect("cancel action should be accepted");
        assert_eq!(id.len(), 32, "correlation id should be 32 hex chars");
        assert!(
            id.chars().all(|c| c.is_ascii_hexdigit()),
            "correlation id should be hex: {id}"
        );
        assert_eq!(plan.initial_status, ActionStatus::Cancelled);
    }

    #[test]
    fn start_publish_action_with_signed_event_is_accepted() {
        // A `PublishAction::Publish` with a non-empty id+sig passes
        // `PublishModule::start`'s validation gate.
        let registry = default_registry();
        let action = crate::publish::PublishAction::Publish {
            handle: "h1".to_string(),
            event: fixture_signed_event(),
            target: crate::publish::PublishTarget::Auto,
        };
        let action_json = serde_json::to_string(&action).unwrap();
        let (id, plan) = registry
            .start(&mut ctx(), "nmp.publish", &action_json)
            .expect("publish action with id+sig should be accepted");
        assert_eq!(id.len(), 32);
        assert_eq!(plan.initial_status, ActionStatus::Pending);
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
        // Valid JSON, wrong shape for `PublishAction` (no Publish/Cancel key).
        let registry = default_registry();
        let err = registry
            .start(&mut ctx(), "nmp.publish", r#"{"t":"PublishNote"}"#)
            .expect_err("wrong-shape JSON must be rejected");
        assert!(matches!(err, ActionRejection::Invalid(_)));
    }

    #[test]
    fn correlation_ids_are_unique_across_calls() {
        let registry = default_registry();
        let action_json = r#"{"Cancel":{"handle":"h"}}"#;
        let mut seen = std::collections::HashSet::new();
        for _ in 0..256 {
            let (id, _) = registry
                .start(&mut ctx(), "nmp.publish", action_json)
                .unwrap();
            assert!(seen.insert(id.clone()), "duplicate correlation id: {id}");
        }
    }
}
