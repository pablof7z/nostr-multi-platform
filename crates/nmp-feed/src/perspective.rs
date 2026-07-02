//! App-registered custom-perspective registry (#1740 step 4).
//!
//! A [`CustomPerspectiveId`] names an app-defined perspective WITHOUT a
//! `Perspective` trait and WITHOUT a native closure crossing FFI. Step 1 minted
//! the opaque id; step 3 left every `Custom` reference fail-closed. This module
//! adds the registration mechanism: an app registers a CLOSED-DATA definition —
//! a [`CustomPerspectiveDef`] — under an id, and the perspective compiler
//! (`explicit composition`) resolves the id back to that definition and compiles it
//! through the SAME step-3 resolver. Nothing app-supplied is invoked; the
//! registry stores only data.
//!
//! # What a definition is (closed data, not a closure)
//!
//! A [`CustomPerspectiveDef`] is:
//! * `source` — a [`FeedScope`] expression (the SAME closed algebra as a
//!   non-custom feed: author sets, `#t` tags, WoT, set algebra). This is what a
//!   `FeedScope::CustomPerspectiveId(id)` source resolves to, and what a
//!   `FeedAdmission::Custom(id)` admission gate compiles from.
//! * `order` — the [`FeedOrder`] the perspective demands. A
//!   `FeedOrder::Custom(id)` resolves to this; if the engine cannot honor it
//!   (only [`FeedOrder::NewestByFeedPosition`] is wired) the open fails closed,
//!   never silently mis-orders.
//!
//! NO closure / trait / raw filter is stored. The definition is pure values an
//! app could equally serialize; the framework resolves them.
//!
//! # Lifetime — register-once, immutable
//!
//! A registration lives for the lifetime of the [`PerspectiveRegistry`] it is
//! stored in. The framework hangs one registry off the app (process-lifetime in
//! practice — it lives as long as the `NmpApp`). A perspective definition is a
//! stable capability an app declares ONCE at startup, like a registered
//! projection: it is **immutable** and not individually retractable.
//!
//! [`PerspectiveRegistry::register`] is therefore register-ONCE — a second
//! registration under an already-registered id is REJECTED (the first definition
//! stands). This is a fail-CLOSED safety property, not a convenience limit: a
//! live feed session captures the COMPILED admission of the definition that
//! existed when it opened. If `register` could overwrite, re-registering a
//! NARROWER gate would leave already-open feeds admitting under the stale WIDER
//! policy — a fail-OPEN leak (D4). Immutability removes that hazard: a
//! definition never changes underneath a running session. (If mutable
//! perspectives are ever needed they require a deliberate versioned
//! recompile/reset-dependents path, not a silent overwrite.)
//!
//! Doctrine map:
//! - D0: the registry names no app product — it stores a [`FeedScope`] /
//!   [`FeedOrder`], both framework-neutral closed data.
//! - D4: it stores a definition only; resolution reuses the ONE step-3 compiler
//!   (`resolve_scope`/`build_scope_session`). There is no second resolver.
//! - D8: the store is a bounded `BTreeMap` keyed by id; a poisoned lock degrades
//!   to "unregistered" (look-up returns `None`) so an open over a poisoned
//!   registry fails CLOSED, never open.

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::params::{CustomPerspectiveId, FeedOrder, FeedScope};

/// The CLOSED-DATA definition of an app-registered custom perspective.
///
/// Stored under a [`CustomPerspectiveId`]; resolved by the perspective compiler.
/// Holds only values — no closure, no trait object, no raw filter (D0).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustomPerspectiveDef {
    /// The acquisition scope this perspective draws from — the SAME closed
    /// [`FeedScope`] algebra a non-custom feed uses. A
    /// `FeedScope::CustomPerspectiveId(id)` source resolves to this; a
    /// `FeedAdmission::Custom(id)` admission gate is the COMPILED admission of
    /// this scope (built inside the framework by the step-3 resolver).
    pub source: FeedScope,
    /// The order this perspective demands. A `FeedOrder::Custom(id)`
    /// resolves to this; the compiler fails closed if the engine cannot honor
    /// it (only [`FeedOrder::NewestByFeedPosition`] is wired) rather than
    /// silently mis-ordering.
    pub order: FeedOrder,
}

impl CustomPerspectiveDef {
    /// A definition over `source` with the default feed-position order (the
    /// only order the engine honors today).
    #[must_use]
    pub fn new(source: FeedScope) -> Self {
        Self {
            source,
            order: FeedOrder::NewestByFeedPosition,
        }
    }

    /// Set this definition's order (builder-style).
    #[must_use]
    pub fn with_order(mut self, order: FeedOrder) -> Self {
        self.order = order;
        self
    }
}

/// A registry of app-declared [`CustomPerspectiveDef`]s, keyed by opaque
/// [`CustomPerspectiveId`].
///
/// The framework hangs one off the app. The perspective compiler reads it to
/// resolve `Custom` references; an UNREGISTERED id resolves to `None`, so the
/// compiler fails CLOSED (no leak). Stores closed data only (D0); is not a feed
/// engine (D4).
#[derive(Default)]
pub struct PerspectiveRegistry {
    defs: Mutex<BTreeMap<CustomPerspectiveId, CustomPerspectiveDef>>,
}

impl PerspectiveRegistry {
    /// Register the definition for `id`, register-ONCE.
    ///
    /// Returns `true` if `id` was newly registered, `false` if it was already
    /// registered (the EXISTING definition stands — registrations are immutable
    /// so a live session never sees its compiled admission change underneath it;
    /// see the module-level fail-open rationale) or if the lock is poisoned (D6 —
    /// best-effort, never panics).
    pub fn register(&self, id: CustomPerspectiveId, def: CustomPerspectiveDef) -> bool {
        let Ok(mut defs) = self.defs.lock() else {
            return false;
        };
        if defs.contains_key(&id) {
            return false; // immutable: the first definition wins, no overwrite.
        }
        defs.insert(id, def);
        true
    }

    /// The definition registered under `id`, or `None` if unregistered.
    ///
    /// `None` is the fail-closed signal the compiler keys on: an unregistered
    /// (or never-successfully-registered) id has no definition, so the open is
    /// rejected and nothing is registered. A poisoned lock also returns `None`
    /// (fail closed, never open).
    #[must_use]
    pub fn get(&self, id: &CustomPerspectiveId) -> Option<CustomPerspectiveDef> {
        self.defs.lock().ok().and_then(|defs| defs.get(id).cloned())
    }

    /// Whether `id` has a registered definition (test/diagnostic).
    #[must_use]
    pub fn is_registered(&self, id: &CustomPerspectiveId) -> bool {
        self.defs
            .lock()
            .map(|defs| defs.contains_key(id))
            .unwrap_or(false)
    }

    /// Count of registered perspectives (test/diagnostic).
    #[must_use]
    pub fn len(&self) -> usize {
        self.defs.lock().map(|defs| defs.len()).unwrap_or(0)
    }

    /// Whether the registry is empty (test/diagnostic).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
#[path = "perspective_tests.rs"]
mod tests;
