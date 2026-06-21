//! Compiled, CLOSED feed-admission predicate (#1740 step 3).
//!
//! This is the *compiled* product of a [`crate::FeedScope`] acquisition
//! expression: a closed, data-driven algebra over already-RESOLVED author sets.
//! The composition layer ([`nmp-defaults`]) resolves a scope's underlying set
//! (the active follows, an owner's kind:3, a NIP-51 list's members, ranked WoT
//! candidates, …) into one of these values; the kernel/engine then derives a
//! [`crate::FollowPredicate`] from it via [`AdmitExpr::to_follow_predicate`].
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

use crate::FollowPredicate;

/// A closed algebra over resolved author sets — the compiled admission
/// predicate a [`crate::FeedScope`] compiles down to.
///
/// Set algebra ([`Self::Or`] / [`Self::And`] / [`Self::AndNot`]) composes the
/// compiled admissions of sub-expressions, matching the `Union` /
/// `Intersection` / `Difference` scope variants respectively.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdmitExpr {
    /// Admit ANY author. Used by acquisition-scoped feeds whose acquisition
    /// filter already does the gating (a `#t` tag scope or a relay-set scope):
    /// every row the acquisition delivers is, by construction, in scope.
    Any,
    /// Admit iff the root author is a member of this explicit, resolved set
    /// (the active follows, a contact list, a list's members, ranked WoT
    /// candidates). An EMPTY set admits nobody — fail-closed (D6).
    Authors(BTreeSet<String>),
    /// Logical OR — the compiled admission of a `Union` scope.
    Or(Box<AdmitExpr>, Box<AdmitExpr>),
    /// Logical AND — the compiled admission of an `Intersection` scope.
    And(Box<AdmitExpr>, Box<AdmitExpr>),
    /// Left AND NOT right — the compiled admission of a `Difference` scope
    /// (admit members of the left set that are not in the right set).
    AndNot(Box<AdmitExpr>, Box<AdmitExpr>),
}

impl AdmitExpr {
    /// A pure membership test on a candidate root-author pubkey (lowercase hex).
    ///
    /// Fully data-driven; no closure, no I/O. Recurses over the set-algebra
    /// combinators.
    #[must_use]
    pub fn matches(&self, author: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Authors(set) => set.contains(author),
            Self::Or(l, r) => l.matches(author) || r.matches(author),
            Self::And(l, r) => l.matches(author) && r.matches(author),
            Self::AndNot(l, r) => l.matches(author) && !r.matches(author),
        }
    }

    /// Derive a [`crate::FollowPredicate`] from this compiled data.
    ///
    /// The returned closure captures an owned clone of `self` and calls
    /// [`Self::matches`]. It is built INSIDE the framework — nothing
    /// app-supplied crosses the seam — so the "no closures across FFI" rule
    /// holds: the app named a typed scope, not a closure. Fail-closed: an
    /// `Authors(∅)` predicate returns `false` for every pubkey.
    #[must_use]
    pub fn to_follow_predicate(self) -> FollowPredicate {
        Arc::new(move |pubkey: &str| self.matches(pubkey))
    }
}

#[cfg(test)]
#[path = "admit_tests.rs"]
mod tests;
