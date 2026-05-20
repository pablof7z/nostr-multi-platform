//! `ActionRegistry` — the runtime that drives the `ActionModule` trait.
//!
//! # What this is (and is NOT)
//!
//! `substrate::ActionModule` has 16+ implementations (`PublishModule`, 15
//! NIP-29 actions, `WelcomeWrapModule`). Until now nothing dispatched into
//! them — `publish/action.rs` carried the note "Wiring of start/reduce into
//! the actor mailbox lands with the kernel action ledger (M6)." This module
//! is the first half of that wiring.
//!
//! This is deliberately NOT the deleted `ModuleRegistry` that
//! `substrate/mod.rs` warns about. That registry "only collected
//! `(namespace, family, type_name)` strings — nothing ever read them back."
//! This registry stores live `dyn ErasedActionModule` trait objects and
//! [`ActionRegistry::start`] actually *invokes* `ActionModule::start`. The
//! read-back path is real: [`crate::ffi`]'s `nmp_app_dispatch_action` calls
//! [`ActionRegistry::start`] and returns the resulting correlation id.
//!
//! # Scope (M6 boundary)
//!
//! This module performs **action validation + correlation-id assignment**
//! and nothing else. It does NOT execute the action:
//!
//! * `PublishModule::start` (see `publish/action.rs`) only checks that the
//!   signed event carries a non-empty `id`/`sig` and returns an
//!   `ActionPlan` — the actual relay dispatch is driven by `PublishEngine`
//!   via `ActorCommand::PublishSignedEvent`, entirely separate from this
//!   path.
//! * Durable persistence + replay of in-flight actions (the action ledger)
//!   is M6 and out of scope here.
//!
//! So `start()` returning a correlation id means "the action was accepted
//! and assigned an id", not "the action ran". Execution wiring is a
//! follow-up.
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

use crate::substrate::{
    ActionContext, ActionId, ActionInput, ActionModule, ActionPlan, ActionRejection,
    ActionTransition,
};

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

    /// Drive one [`ActionModule::reduce`] step with erased input/output.
    #[allow(dead_code)] // Wired for completeness; the M6 ledger is the caller.
    fn reduce(
        &self,
        ctx: &mut ActionContext,
        id: ActionId,
        input: ActionInput<Value>,
    ) -> ActionTransition<Value, Value>;
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

    fn reduce(
        &self,
        ctx: &mut ActionContext,
        id: ActionId,
        input: ActionInput<Value>,
    ) -> ActionTransition<Value, Value> {
        // Translate the erased input back into the module's typed `Step`.
        let typed_input = match input {
            ActionInput::Started => ActionInput::Started,
            ActionInput::ResumedAfterRestart { step } => {
                match serde_json::from_value::<M::Step>(step) {
                    Ok(s) => ActionInput::ResumedAfterRestart { step: s },
                    Err(e) => {
                        return ActionTransition::Fail {
                            reason: format!("step deserialize: {e}"),
                            transient: false,
                        };
                    }
                }
            }
            ActionInput::CapabilityResult { value } => ActionInput::CapabilityResult { value },
            ActionInput::RelayOk { relay_url } => ActionInput::RelayOk { relay_url },
            ActionInput::Timeout => ActionInput::Timeout,
            ActionInput::Cancel => ActionInput::Cancel,
        };
        let transition = M::reduce(ctx, id, typed_input);
        // Erase the typed transition back to `serde_json::Value`.
        match transition {
            ActionTransition::Continue { step, status } => ActionTransition::Continue {
                step: serde_json::to_value(&step).unwrap_or(Value::Null),
                status,
            },
            ActionTransition::Complete { output } => ActionTransition::Complete {
                output: serde_json::to_value(&output).unwrap_or(Value::Null),
            },
            ActionTransition::Fail { reason, transient } => {
                ActionTransition::Fail { reason, transient }
            }
            ActionTransition::AwaitCapability {
                request_namespace,
                payload,
                next_step,
            } => ActionTransition::AwaitCapability {
                request_namespace,
                payload,
                next_step: serde_json::to_value(&next_step).unwrap_or(Value::Null),
            },
            ActionTransition::AwaitUserApproval { prompt, next_step } => {
                ActionTransition::AwaitUserApproval {
                    prompt,
                    next_step: serde_json::to_value(&next_step).unwrap_or(Value::Null),
                }
            }
        }
    }
}

/// Namespace-keyed registry of [`ActionModule`]s.
///
/// Stateless apart from the module table: every registered module is a ZST
/// adapter, so the registry is cheap to construct and `Send + Sync` (the
/// adapters are too). [`Self::start`] is the live dispatch path — it
/// validates an action and assigns a correlation id; see the module-level
/// docs for the M6 scope boundary.
pub struct ActionRegistry {
    modules: HashMap<&'static str, Box<dyn ErasedActionModule>>,
}

impl Default for ActionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionRegistry {
    /// An empty registry. Call [`Self::register`] for each module.
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
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

    /// Validate `action_json` against the module registered under
    /// `namespace`, returning a fresh correlation id plus the erased
    /// [`ActionPlan`].
    ///
    /// An unknown namespace is an [`ActionRejection::Invalid`]; a JSON shape
    /// that does not match the module's `Action` type is also
    /// `ActionRejection::Invalid` (surfaced from the adapter). The
    /// correlation id is generated *after* validation succeeds so a rejected
    /// action never consumes one.
    ///
    /// NOTE: per the M6 scope boundary (module docs), a returned id means
    /// the action was accepted, not executed.
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

    /// Drive one reduce step for an in-flight action. Returns `None` when
    /// `namespace` is not registered.
    ///
    /// Wired for completeness — the M6 action ledger is the intended caller.
    /// No code drives `reduce` today (the publish engine drives transitions
    /// in-process), so this is `#[allow(dead_code)]`.
    #[allow(dead_code)]
    pub fn reduce(
        &self,
        ctx: &mut ActionContext,
        namespace: &str,
        id: ActionId,
        input: ActionInput<Value>,
    ) -> Option<ActionTransition<Value, Value>> {
        let module = self.modules.get(namespace)?;
        Some(module.reduce(ctx, id, input))
    }

    /// `true` when a module is registered under `namespace`.
    #[allow(dead_code)] // Test/diagnostic helper.
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
/// against its own registry instance.
pub fn default_registry() -> ActionRegistry {
    let mut registry = ActionRegistry::new();
    registry.register::<crate::publish::PublishModule>();
    registry
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
