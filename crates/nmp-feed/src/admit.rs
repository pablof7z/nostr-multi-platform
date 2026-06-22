//! Compiled, CLOSED feed-admission predicate (#1740 step 3).
//!
//! This is the *compiled* product of a [`crate::FeedScope`] acquisition
//! expression: a closed, data-driven algebra over already-RESOLVED author sets.
//! The composition layer ([`nmp-defaults`]) resolves a scope's underlying set
//! (the active follows, an owner's kind:3, a NIP-51 list's members, ranked WoT
//! candidates, …) into one of these values; the kernel/engine then derives a
//! [`crate::RootAdmission`] from it via [`AdmitExpr::to_root_admission`].
//!
//! Why DATA and not a closure: the perspective compiler is CLOSED. No
//! app-defined closure crosses the FFI boundary — the app names a typed scope,
//! the framework resolves it to a pubkey set, and the admission predicate is
//! this pure value. The `FollowPredicate` closure is constructed INSIDE the
//! framework from this data; nothing app-supplied is invoked.
//!
//! Doctrine map:
//! - D0: the type names no app noun — it is a set algebra over hex pubkeys.
//! - D4: there is one admission model; the engine's `FollowPredicate` is derived
//!   from it, not a second parallel predicate.
//! - D6: fail-closed. An empty [`AdmitExpr::Authors`] admits NOBODY (a resolved
//!   set that came back empty must not silently admit everyone).

use std::collections::BTreeSet;
use std::sync::Arc;

use nmp_core::substrate::KernelEvent;

use crate::RootAdmission;

/// A closed, EVENT-AWARE algebra — the compiled admission predicate a
/// [`crate::FeedScope`] compiles down to.
///
/// It is event-aware (not author-only) so author-scope and `#t` tag-scope
/// perspectives compose FAITHFULLY under set algebra. Without this,
/// `Intersection(Tag, ContactList)` could only be expressed by treating the tag
/// scope as [`Self::Any`] inside the AND — silently mis-admitting any member's
/// untagged note. With the [`Self::Tag`] variant the intersection checks BOTH
/// the event's author membership AND its `#t` tags.
///
/// Set algebra ([`Self::Or`] / [`Self::And`] / [`Self::AndNot`]) composes the
/// compiled admissions of sub-expressions, matching the `Union` /
/// `Intersection` / `Difference` scope variants respectively.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdmitExpr {
    /// Admit ANY event. Used by relay-set scopes whose acquisition routing does
    /// the gating: every row the acquisition delivers is, by construction, in
    /// scope. (A `#t` tag scope now compiles to [`Self::Tag`], not `Any`, so it
    /// composes correctly inside set algebra.)
    Any,
    /// Admit iff the event's author is a member of this explicit, resolved set
    /// (the active follows, a contact list, a list's members, ranked WoT
    /// candidates). An EMPTY set admits nobody — fail-closed (D6).
    Authors(BTreeSet<String>),
    /// Admit iff the event carries a `#t` tag equal to this term (the compiled
    /// admission of a `Tag` scope). Event-aware so it composes with author
    /// scopes under set algebra.
    Tag(String),
    /// Logical OR — the compiled admission of a `Union` scope.
    Or(Box<AdmitExpr>, Box<AdmitExpr>),
    /// Logical AND — the compiled admission of an `Intersection` scope.
    And(Box<AdmitExpr>, Box<AdmitExpr>),
    /// Left AND NOT right — the compiled admission of a `Difference` scope
    /// (admit events admitted by the left expr but not the right).
    AndNot(Box<AdmitExpr>, Box<AdmitExpr>),
}

impl AdmitExpr {
    /// A pure admission test on a candidate ROOT event.
    ///
    /// Fully data-driven; no closure, no I/O. Recurses over the set-algebra
    /// combinators, checking the event's author for [`Self::Authors`] and its
    /// `#t` tags for [`Self::Tag`].
    #[must_use]
    pub fn matches(&self, event: &KernelEvent) -> bool {
        match self {
            Self::Any => true,
            Self::Authors(set) => set.contains(&event.author),
            Self::Tag(term) => event_has_t_tag(event, term),
            Self::Or(l, r) => l.matches(event) || r.matches(event),
            Self::And(l, r) => l.matches(event) && r.matches(event),
            Self::AndNot(l, r) => l.matches(event) && !r.matches(event),
        }
    }

    /// Derive a [`crate::RootAdmission`] from this compiled data.
    ///
    /// The returned closure captures an owned clone of `self` and calls
    /// [`Self::matches`]. It is built INSIDE the framework — nothing
    /// app-supplied crosses the seam — so the "no closures across FFI" rule
    /// holds: the app named a typed scope, not a closure. Fail-closed: an
    /// `Authors(∅)` predicate returns `false` for every event.
    #[must_use]
    pub fn to_root_admission(self) -> RootAdmission {
        Arc::new(move |event: &KernelEvent| self.matches(event))
    }
}

/// `true` iff `event` carries a `#t` tag whose value equals `term`.
fn event_has_t_tag(event: &KernelEvent, term: &str) -> bool {
    event.tags.iter().any(|tag| {
        tag.first().is_some_and(|name| name == "t") && tag.get(1).is_some_and(|val| val == term)
    })
}

#[cfg(test)]
#[path = "admit_tests.rs"]
mod tests;
