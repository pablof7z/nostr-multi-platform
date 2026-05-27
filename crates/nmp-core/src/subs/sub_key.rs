//! `SubKey`, `SubOwnerKey`, `SubScope`, and the `(owner, key, scope)`
//! subscription-identity triple.
//!
//! Ported from notedeck's subscription-runtime distillation in
//! `docs/design/nostrdb-notedeck-lessons.md` §3.1, §3.2, §3.4:
//!
//! - **`SubKey`** (§3.1) — hashed typed *stable* identity for a logical
//!   subscription. Built by folding typed parts into a hasher (the
//!   `egui::Id` pattern). No string formatting, no allocation, comparable,
//!   hashable, stable across restarts and processes.
//! - **`SubOwnerKey`** (§3.2) — the UI-lifecycle anchor. Many owners may
//!   attach to the same `(scope, key)`; the registry keeps the logical
//!   interest alive while any owner is attached and drops it when the last
//!   owner leaves (multi-owner sharing / dedup-across-owners).
//! - **`SubScope`** (§3.4) — `Account(pubkey)` vs `Global`. Two interests
//!   with the same `SubKey` under different scopes are *distinct* entries
//!   (account-scoped vs global-scoped isolation).
//!
//! Doctrine: D4 (the [`crate::subs::InterestRegistry`] remains the single
//! writer keyed by this triple), D8 (these are cheap stack values; building
//! a `SubKey` allocates nothing and the registry's snapshot order stays
//! deterministic).

use std::hash::{Hash, Hasher};

use crate::stable_hash::{stable_hash64, StableHasher};

use crate::planner::Pubkey;

// ─── SubKey ──────────────────────────────────────────────────────────────────

/// Stable identity for a *logical* subscription, constructed by hashing typed
/// tuples. Borrowed from `egui::Id` via notedeck (`nostrdb-notedeck-lessons.md`
/// §3.1). No string formatting, no allocation; `Copy`, comparable, hashable.
///
/// The same logical subscription (e.g. "the thread for event X") hashes to the
/// same `SubKey` across restarts and processes, which is what gives the action
/// ledger and ADR-0007 diagnostics a stable handle.
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct SubKey(pub u64);

impl SubKey {
    /// Hash any single `Hash`-able value into a `SubKey`.
    #[must_use]
    pub fn new(value: impl Hash) -> Self {
        Self(stable_hash64(value))
    }

    /// Start an incremental builder seeded with `seed`. Fold further parts in
    /// with [`SubKeyBuilder::with`], then [`SubKeyBuilder::finish`].
    #[must_use]
    pub fn builder(seed: impl Hash) -> SubKeyBuilder {
        let mut h = StableHasher::new();
        seed.hash(&mut h);
        SubKeyBuilder { hasher: h }
    }
}

/// Incremental [`SubKey`] builder (the `egui::Id` builder shape, §3.1).
pub struct SubKeyBuilder {
    hasher: StableHasher,
}

impl SubKeyBuilder {
    /// Fold another typed part into the running hash.
    #[must_use]
    pub fn with(mut self, part: impl Hash) -> Self {
        part.hash(&mut self.hasher);
        self
    }

    /// Finalize the running hash into a [`SubKey`].
    #[must_use]
    pub fn finish(self) -> SubKey {
        SubKey(self.hasher.finish())
    }
}

// ─── SubOwnerKey ─────────────────────────────────────────────────────────────

/// The UI-lifecycle anchor for a subscription (`nostrdb-notedeck-lessons.md`
/// §3.2). One owner per route/view instance. Multiple owners may attach to the
/// same `(scope, key)`; the registry keeps the logical interest alive while
/// any owner is attached and drops it when the last owner leaves.
///
/// Built the same hashed-typed way as [`SubKey`] so an owner can be derived
/// deterministically from a view-instance descriptor without allocation.
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct SubOwnerKey(pub u64);

impl SubOwnerKey {
    /// Hash any single `Hash`-able value into a `SubOwnerKey`.
    #[must_use]
    pub fn new(value: impl Hash) -> Self {
        Self(stable_hash64(value))
    }
}

// ─── SubScope ────────────────────────────────────────────────────────────────

/// Account-context scope for a subscription (`nostrdb-notedeck-lessons.md`
/// §3.4). `Account` is resolved to a concrete pubkey; `Global` is not tied to
/// any account.
///
/// Scope is part of the registry key: the *same* [`SubKey`] under
/// `Account(alice)` and under `Global` are two distinct registry entries
/// (account-scoped vs global-scoped isolation). M8's switch-away/restore
/// lifecycle keys off `SubScope::Account`.
#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub enum SubScope {
    /// Bound to a concrete account pubkey (hex). Re-routes / sleeps on
    /// account switch (M8).
    Account(Pubkey),
    /// Not tied to any account (global pointer loaders, indexer probes).
    Global,
}

// ─── SubIdentity ─────────────────────────────────────────────────────────────

/// The `(owner, key, scope)` ownership triple — the registry's primary key
/// (`nostrdb-notedeck-lessons.md` §3.2).
///
/// `Ord` orders by `(scope, key, owner)` so that snapshots iterate
/// scope-then-key-major (deterministic; D8). The registry dedups the live
/// interest across owners sharing the same `(scope, key)`.
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct SubIdentity {
    /// UI-lifecycle anchor — one per route/view instance.
    pub owner: SubOwnerKey,
    /// Logical subscription identity (stable across restarts).
    pub key: SubKey,
    /// Account-context scope.
    pub scope: SubScope,
}

impl SubIdentity {
    /// Construct a triple.
    #[must_use]
    pub fn new(owner: SubOwnerKey, key: SubKey, scope: SubScope) -> Self {
        Self { owner, key, scope }
    }

    /// The `(scope, key)` pair shared across owners — the dedup key.
    pub(crate) fn shared(&self) -> (SubScope, SubKey) {
        (self.scope.clone(), self.key)
    }
}

impl PartialOrd for SubIdentity {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SubIdentity {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // scope-major, then key, then owner — deterministic snapshot order.
        self.scope
            .cmp(&other.scope)
            .then(self.key.cmp(&other.key))
            .then(self.owner.cmp(&other.owner))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sub_key_is_deterministic_and_stable() {
        assert_eq!(SubKey::new("thread:abc"), SubKey::new("thread:abc"));
        assert_ne!(SubKey::new("thread:abc"), SubKey::new("thread:xyz"));
        assert_eq!(SubKey::new("thread:abc"), SubKey(0x3ac9_0cf9_3fcc_3690));
    }

    #[test]
    fn sub_key_builder_folds_parts() {
        let a = SubKey::builder("thread").with(42u64).with("root").finish();
        let b = SubKey::builder("thread").with(42u64).with("root").finish();
        let c = SubKey::builder("thread").with(43u64).with("root").finish();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn scope_distinguishes_same_key() {
        let k = SubKey::new("profile:alice");
        let acct = SubIdentity::new(SubOwnerKey::new("v1"), k, SubScope::Account("alice".into()));
        let glob = SubIdentity::new(SubOwnerKey::new("v1"), k, SubScope::Global);
        assert_ne!(acct.shared(), glob.shared());
    }

    #[test]
    fn owners_share_scope_key_pair() {
        let k = SubKey::new("profile:alice");
        let o1 = SubIdentity::new(SubOwnerKey::new("v1"), k, SubScope::Global);
        let o2 = SubIdentity::new(SubOwnerKey::new("v2"), k, SubScope::Global);
        assert_eq!(o1.shared(), o2.shared());
        assert_ne!(o1, o2);
    }
}
