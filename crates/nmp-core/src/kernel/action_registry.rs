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
//! read-back path is real: `nmp_ffi`'s `nmp_app_dispatch_action_bytes` calls
//! [`ActionRegistry::start`] and returns the resulting correlation id.
//!
//! # Scope (validation + execution, both in the registry)
//!
//! This registry performs **action validation, correlation-id assignment,
//! AND execution dispatch**:
//!
//! * [`ActionRegistry::start`] validates and assigns a correlation id.
//! * [`ActionRegistry::execute`] drives the validated action to the actor by
//!   calling `M::execute` through the dyn-safe [`ErasedActionModule`] facade.
//!   Each module is registered once via [`ActionRegistry::register::<M>`];
//!   no separate executor seam exists (ADR-0071).
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
use std::sync::Arc;

use super::composition_ledger::{CompositionLedger, Disposition};
use crate::substrate::{
    ActionContext, ActionId, ActionModule, ActionRegistrar, ActionRejection, ActionResult,
};

mod action_id;
mod erased;
mod execute;
mod failure;
mod result_observer;
mod typed_dispatch;

use action_id::new_action_id;
use erased::{ActionModuleAdapter, ErasedActionModule};
pub use failure::{ActionExecuteFailure, ActionFailureKind, RegistrationError};
use result_observer::{new_result_observer_slot, ResultObserverSlot};

/// Per-namespace provenance: did the live entry come from a yielding default
/// or from an explicit app registration? (ADR-0069 Part 1.)
///
/// The distinction drives two behaviours:
/// * [`ActionRegistry::register_default`] yields (declines to install) when the
///   namespace is already claimed — by EITHER provenance.
/// * [`ActionRegistry::register`] (the app path) loudly fails when it replaces
///   an [`Provenance::App`] entry — an app-over-app collision is a composition
///   bug — while silently overriding a [`Provenance::Default`] entry (an app
///   intentionally replacing a default is legal).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Provenance {
    /// Installed via the yielding-default path (`register_default`).
    Default,
    /// Installed via the explicit app path (`register`).
    App,
}

/// Namespace-keyed registry of [`ActionModule`]s.
///
/// Stateless apart from the modules table: every registered module is a ZST
/// adapter (cheap, `Send + Sync`). [`Self::start`] validates and assigns a
/// correlation id; [`Self::execute`] drives the validated action to the actor
/// via the same module's `execute()`. A module with no entry in the table
/// returns `Err("unknown action namespace …")` from `start` and `Err("no
/// executor registered for namespace '…'")` from `execute` — the caller
/// surfaces these as `{"error":…}` (D6).
pub struct ActionRegistry {
    modules: HashMap<String, Box<dyn ErasedActionModule>>,
    /// Per-namespace provenance + the registering provider's type name
    /// (ADR-0069 Part 1). Keyed identically to `modules`; an entry is present
    /// iff `modules` holds the namespace. The `provider` string feeds the
    /// composition ledger's `replaced` field and the app-over-app collision
    /// diagnostic.
    provenance: HashMap<String, (Provenance, &'static str)>,
    /// Optional composition ledger (ADR-0069 Part 2). `None` for a bare
    /// registry (the kernel's `default_registry` and most unit tests); the
    /// host wires a shared `Arc<CompositionLedger>` via
    /// [`Self::with_composition_ledger`] so registration decisions are
    /// recorded for `nmp_app_composition_report`.
    ledger: Option<Arc<CompositionLedger>>,
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
    /// An empty registry. Call [`Self::register`] for each module.
    #[must_use]
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
            provenance: HashMap::new(),
            ledger: None,
            result_observer: new_result_observer_slot(),
        }
    }

    /// Attach a shared composition ledger so registration decisions are
    /// recorded for `nmp_app_composition_report` (ADR-0069 Part 2).
    ///
    /// Builder-style: the host calls this once, right after `ActionRegistry::new`,
    /// before any registration. A registry with no ledger records nothing — the
    /// yielding/override semantics are identical either way.
    #[must_use]
    pub fn with_composition_ledger(mut self, ledger: Arc<CompositionLedger>) -> Self {
        self.ledger = Some(ledger);
        self
    }

    /// Register module `M` under its [`ActionModule::NAMESPACE`] via the **app
    /// path** — an explicit, intentional registration (ADR-0069 Part 1).
    ///
    /// Semantics by what currently holds the namespace:
    /// * **unclaimed** → install `M` ([`Disposition::Installed`]).
    /// * **held by a yielding default** → override it ([`Disposition::ReplacedPrevious`]).
    ///   An app replacing a default is legal and expected (the Bevy/Spring
    ///   "bring your own bean" case).
    /// * **held by another app registration** → an app-over-app collision. This
    ///   is a composition bug: a hard `debug_assert!` failure in dev/test builds,
    ///   recorded-but-soft in release (D6 — no panic across the C-ABI). The new
    ///   module still replaces the old (last-writer-wins) so release behaviour
    ///   is unchanged from before ADR-0069; the ledger makes the collision
    ///   visible either way.
    ///
    /// `M::start` handles validation and `M::execute` handles execution — both
    /// under the same `M::NAMESPACE`, so namespace mismatch between validator
    /// and executor is structurally impossible (ADR-0071).
    ///
    /// ADR-0072 rung 5.2: takes the module **value** so a stateful module
    /// stores its captured dependencies in the registry.
    ///
    /// Returns `Ok(())` on success. Returns `Err(`[`RegistrationError`]`)` when
    /// the namespace is ALREADY held by another **app** registration — an
    /// app-over-app collision (ADR-0069). The new module still replaces the old
    /// (last-writer-wins for release resilience, D6 — no panic across the
    /// C-ABI); the error is returned so the caller can surface it in BOTH dev
    /// AND release builds (#1724). Overriding a yielding default is legal and
    /// does NOT return an error.
    pub fn register<M: ActionModule + 'static>(
        &mut self,
        module: M,
    ) -> Result<(), RegistrationError> {
        let provider = std::any::type_name::<M>();
        let namespace = M::NAMESPACE.as_str();
        let prior = self.provenance.get(namespace).copied();
        let (disposition, collision) = match prior {
            None => (Disposition::Installed, None),
            Some((Provenance::Default, _)) => (Disposition::ReplacedPrevious, None),
            Some((Provenance::App, prev_provider)) => {
                // App-over-app collision: structured error in both dev and release.
                let err = RegistrationError {
                    namespace,
                    prior_provider: prev_provider,
                    new_provider: provider,
                };
                (Disposition::ReplacedPrevious, Some(err))
            }
        };
        let replaced = prior.map(|(_, prev_provider)| prev_provider.to_string());

        self.modules
            .insert(namespace.to_string(), Box::new(ActionModuleAdapter(module)));
        self.provenance
            .insert(namespace.to_string(), (Provenance::App, provider));

        if let Some(ledger) = &self.ledger {
            ledger.record(
                "action_registry",
                namespace,
                provider,
                disposition,
                replaced,
            );
        }

        match collision {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    /// Register module `M` as a **yielding default** under its
    /// [`ActionModule::NAMESPACE`] (ADR-0069 Part 1).
    ///
    /// Entry-or-insert: install `M` ONLY if the namespace is unclaimed. If any
    /// registration (app OR an earlier default) already holds the namespace,
    /// this YIELDS — the existing module stays, `M` is dropped — and returns
    /// `false`. Returns `true` when `M` was installed.
    ///
    /// This is the Spring-Boot `@ConditionalOnMissingBean` shape: a framework
    /// default that an app can pre-empt REGARDLESS of call order. Because the
    /// default yields rather than clobbers, an app registering its own module
    /// under a default namespace BEFORE `explicit owner composition` runs is no longer
    /// silently overwritten — the inverted, order-dependent behaviour ADR-0069
    /// fixes.
    ///
    /// ADR-0072 rung 5.2: takes the module **value**. When the namespace is
    /// already claimed the value is dropped (the existing registration wins),
    /// exactly as before — only the storage shape changed.
    pub fn register_default<M: ActionModule + 'static>(&mut self, module: M) -> bool {
        let provider = std::any::type_name::<M>();
        let namespace = M::NAMESPACE.as_str();
        if let Some((_, existing_provider)) = self.provenance.get(namespace).copied() {
            // Already claimed — yield. Record the yield for the report.
            if let Some(ledger) = &self.ledger {
                ledger.record(
                    "action_registry",
                    namespace,
                    provider,
                    Disposition::YieldedToExisting,
                    Some(existing_provider.to_string()),
                );
            }
            return false;
        }
        self.modules
            .insert(namespace.to_string(), Box::new(ActionModuleAdapter(module)));
        self.provenance
            .insert(namespace.to_string(), (Provenance::Default, provider));
        if let Some(ledger) = &self.ledger {
            ledger.record(
                "action_registry",
                namespace,
                provider,
                Disposition::Installed,
                None,
            );
        }
        true
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
    /// The returned id is a freshly minted [`new_action_id`] — the operation's
    /// sole identity. It is threaded onto the executor's `ActorCommand` and is
    /// the identifier the publish engine reports in `action_results`, so a host
    /// keying a UI spinner on this returned `correlation_id` matches the
    /// terminal verdict to its dispatch. The `correlation_id` is NEVER replaced
    /// with output data such as a pre-signed event's `id` (the event id is the
    /// operation's *result*, not its identity).
    ///
    /// `now_ms` is the caller-supplied wall-clock millisecond stamp. The FFI
    /// dispatch path reads it at the system boundary (not inside the reducer)
    /// so tests can inject a deterministic value.
    pub fn start(
        &self,
        ctx: &mut ActionContext,
        now_ms: u64,
        namespace: &str,
        action_json: &str,
    ) -> Result<ActionId, ActionRejection> {
        let module = self.modules.get(namespace).ok_or_else(|| {
            ActionRejection::Invalid(format!("unknown action namespace: {namespace}"))
        })?;
        // D6: the typed `M::start` body runs on the `nmp_app_dispatch_action`
        // call path (an `extern "C"` function). An unguarded panic would
        // unwind across the FFI boundary (undefined behaviour); a caught
        // panic surfaces as `ActionRejection::Invalid("action validator
        // panicked")` instead.
        match catch_unwind(AssertUnwindSafe(|| module.start(ctx, action_json))) {
            Ok(result) => result?,
            Err(_) => {
                return Err(ActionRejection::Invalid(
                    "action validator panicked".to_string(),
                ));
            }
        };
        // The correlation_id is minted here — AFTER validation succeeds — and is
        // the operation's sole identity. It is never substituted with output
        // data (e.g. a pre-signed event's id); the event id is the operation's
        // result, surfaced through `action_results`, not its identity.
        Ok(new_action_id(now_ms))
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
    /// registration replaces the first. Replacement waits for any in-flight
    /// delivery to finish before returning, so a host may release the previous
    /// callback context immediately after the setter returns.
    ///
    /// Re-entrancy matches the update/capability callback gates: observer code
    /// must not call this setter for the same registry from inside the callback.
    pub fn set_result_observer(&self, f: impl Fn(ActionResult) + Send + Sync + 'static) {
        self.result_observer.set_observer(Arc::new(f));
    }

    /// Clear the host-supplied action-result observer and wait for all
    /// in-flight callbacks to drain before returning.
    pub fn clear_result_observer(&self) {
        self.result_observer.clear_observer();
    }

    /// Deliver `result` to the registered observer, if any.
    ///
    /// A no-op when no observer is registered. The observer is copied and
    /// `in_flight` is incremented under the slot lock, then host code is called
    /// without holding that lock. Clear/replace waits for `in_flight == 0`.
    ///
    /// D6: the observer is untrusted host plugin code registered via the
    /// native action-result observer surface, and this runs on the dispatch
    /// call path. An
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
        self.result_observer.deliver(result);
    }

    /// `true` when a module is registered under `namespace`.
    #[cfg(test)]
    pub fn contains(&self, namespace: &str) -> bool {
        self.modules.contains_key(namespace)
    }

    /// Intrinsic typed-only gate (ADR-0071 / #1756): sorted namespaces of
    /// registered modules NOT typed-capable. See [`erased::untyped_namespaces`].
    #[must_use]
    pub fn untyped_namespaces(&self) -> Vec<String> {
        erased::untyped_namespaces(&self.modules)
    }
}

impl ActionRegistrar for ActionRegistry {
    fn register_action<M: ActionModule + 'static>(
        &mut self,
        module: M,
    ) -> Result<(), RegistrationError> {
        self.register(module)
    }

    fn register_default_action<M: ActionModule + 'static>(&mut self, module: M) -> bool {
        self.register_default(module)
    }
}

/// Build the registry the kernel ships with.
///
/// Always registers [`crate::publish::PublishModule`]. NIP-specific action
/// modules (NIP-17 DM, NIP-29 group, NIP-47 wallet `pay_invoice`, NIP-57
/// zap, …) are *app* nouns (D0 — `nmp-core` never names a protocol crate);
/// the app host registers those against its own registry instance via
/// [`ActionRegistry::register`]. Post-V-38 the `nmp.wallet.pay_invoice`
/// module lives in `nmp-nip47` and the host crate registers it from there.
pub fn default_registry() -> ActionRegistry {
    let mut registry = ActionRegistry::new();
    // `PublishModule` is the only built-in — no collision is possible, so the
    // Result is always Ok. `let _ =` suppresses the must-use warning (#1724).
    let _ = registry.register(crate::publish::PublishModule);
    registry
}

#[cfg(test)]
#[path = "action_registry/lifecycle_tests.rs"]
mod lifecycle_tests;
#[cfg(test)]
#[path = "action_registry/registration_error_tests.rs"]
mod registration_error_tests;
#[cfg(test)]
#[path = "action_registry/terminal_correctness_tests.rs"]
mod terminal_correctness_tests;
#[cfg(test)]
#[path = "action_registry/tests.rs"]
mod tests;
#[cfg(test)]
#[path = "action_registry/typed_dispatch_tests.rs"]
mod typed_dispatch_tests;
