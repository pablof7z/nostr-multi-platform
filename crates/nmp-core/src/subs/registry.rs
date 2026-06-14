//! Logical-interest registry — the single writer of the active-interest set (D4).
//!
//! View modules and action modules push `LogicalInterest`s here; the planner
//! reads via [`InterestRegistry::iter_active`]. The registry is keyed by the
//! `(owner, key, scope)` triple from `docs/design/nostrdb-notedeck-lessons.md`
//! §3.2 (see [`crate::subs::sub_key`]):
//!
//! - **Dedup across owners.** Many owners may attach to the same
//!   `(scope, key)`; the registry keeps *one* live [`LogicalInterest`] per
//!   `(scope, key)` and refcounts owners. The interest stays alive while any
//!   owner is attached and is dropped when the last owner leaves.
//! - **`ensure` vs `set`** (§3.3). [`InterestRegistry::ensure_sub`] is
//!   idempotent register-if-absent: it attaches the owner and, only if the
//!   `(scope, key)` is *absent*, installs the interest — a re-mount never
//!   clobbers an existing filter. [`InterestRegistry::set_sub`] is upsert: it
//!   attaches the owner and *replaces* the interest for `(scope, key)`.
//! - **Account vs global isolation** (§3.4). The same [`SubKey`] under
//!   `SubScope::Account(pubkey)` and `SubScope::Global` are distinct entries.
//!
//! D4: this is the authoritative active set; the planner reads via
//! [`InterestRegistry::iter_active`] but never mutates. Snapshots are
//! deterministically ordered by `(scope, key)` so plan-ids stay stable
//! across recompilations (D8 — no reactivity regression).
//!
//! The legacy `InterestId`-keyed [`InterestRegistry::push`] /
//! [`InterestRegistry::withdraw`] surface is preserved verbatim for existing
//! callers; it is expressed in terms of the triple via a synthetic owner so
//! behaviour is unchanged.

use std::collections::BTreeMap;

use crate::planner::{InterestId, LogicalInterest};
use crate::subs::sub_key::{SubIdentity, SubKey, SubOwnerKey, SubScope};

/// One `(scope, key)` slot: the single live interest plus the set of owners
/// keeping it alive (dedup across owners).
struct Slot {
    interest: LogicalInterest,
    owners: std::collections::BTreeSet<SubOwnerKey>,
}

/// Single-writer registry of active logical interests, keyed by the
/// `(owner, key, scope)` triple with dedup across owners.
#[derive(Default)]
pub struct InterestRegistry {
    /// Live interests keyed by the shared `(scope, key)` pair. `BTreeMap`
    /// keeps the snapshot deterministically ordered (D8).
    slots: BTreeMap<(SubScope, SubKey), Slot>,
}

impl InterestRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    // ─── (owner, key, scope) API — notedeck §3.2/§3.3 ────────────────────────

    /// Idempotent register-if-absent (`ensure_sub`, §3.3).
    ///
    /// Attaches `identity.owner` to the `(scope, key)` slot. If the slot is
    /// **absent**, installs `interest`. If the slot already exists, the
    /// existing interest is left untouched (a re-mount never clobbers an
    /// existing filter — the real bug class §3.3 calls out).
    ///
    /// Returns `true` iff the interest was newly installed.
    #[must_use]
    pub fn ensure_sub(&mut self, identity: SubIdentity, interest: LogicalInterest) -> bool {
        let shared = identity.shared();
        if let Some(slot) = self.slots.get_mut(&shared) {
            slot.owners.insert(identity.owner);
            false
        } else {
            let mut owners = std::collections::BTreeSet::new();
            owners.insert(identity.owner);
            self.slots.insert(shared, Slot { interest, owners });
            true
        }
    }

    /// Upsert (`set_sub`, §3.3).
    ///
    /// Attaches `identity.owner` to the `(scope, key)` slot and *replaces*
    /// the interest (use when filters change mid-life, e.g. a search query
    /// updating as the user types). Owners already attached stay attached.
    pub fn set_sub(&mut self, identity: SubIdentity, interest: LogicalInterest) {
        let shared = identity.shared();
        if let Some(slot) = self.slots.get_mut(&shared) {
            slot.owners.insert(identity.owner);
            slot.interest = interest;
        } else {
            let mut owners = std::collections::BTreeSet::new();
            owners.insert(identity.owner);
            self.slots.insert(shared, Slot { interest, owners });
        }
    }

    /// Detach one owner from its `(scope, key)` slot. When the last owner
    /// leaves, the live interest is dropped (multi-owner GC, §3.2).
    ///
    /// Returns `true` iff the slot was removed (last owner left).
    #[must_use]
    pub fn drop_owner(&mut self, identity: &SubIdentity) -> bool {
        let shared = identity.shared();
        let Some(slot) = self.slots.get_mut(&shared) else {
            return false;
        };
        slot.owners.remove(&identity.owner);
        if slot.owners.is_empty() {
            self.slots.remove(&shared);
            true
        } else {
            false
        }
    }

    /// Owner refcount for a `(scope, key)` slot (diagnostics / tests).
    #[allow(dead_code)]
    #[must_use]
    pub fn owner_count(&self, scope: &SubScope, key: &SubKey) -> usize {
        self.slots
            .get(&(scope.clone(), *key))
            .map_or(0, |s| s.owners.len())
    }

    // ─── Legacy InterestId surface — preserved verbatim (D4) ─────────────────

    /// Push or replace an interest keyed by its `InterestId` (legacy surface).
    ///
    /// Expressed via the triple: the `InterestId` becomes the [`SubKey`], the
    /// interest's [`crate::planner::InterestScope`] maps to [`SubScope`], and a
    /// single synthetic owner keeps it alive. Replacing an existing id with a
    /// new shape is the legal way to mutate an interest's filter — identical
    /// behaviour to the pre-triple registry (single writer; D4).
    pub fn push(&mut self, interest: LogicalInterest) {
        let identity = Self::legacy_identity(&interest);
        self.set_sub(identity, interest);
    }

    /// Withdraw an interest by id (legacy surface). No-op if absent.
    pub fn withdraw(&mut self, id: &InterestId) {
        let key = Self::legacy_key(id);
        // The id alone does not name a scope; withdraw the slot under every
        // scope it may have been registered with. In practice an `InterestId`
        // is registered under exactly one scope, so at most one slot matches.
        self.slots.retain(|(_, k), _| *k != key);
    }

    /// Snapshot of all active interests, deterministically ordered by
    /// `(scope, key)`. Dedup across owners: exactly one interest per
    /// `(scope, key)` regardless of how many owners are attached.
    #[must_use]
    pub fn iter_active(&self) -> Vec<LogicalInterest> {
        self.slots.values().map(|s| s.interest.clone()).collect()
    }

    /// Snapshot of `(SubKey, LogicalInterest)` pairs for every active slot,
    /// deterministically ordered by `(scope, key)`.
    ///
    /// The `SubKey` is the slot's registration key — the SAME key the cache-serve
    /// path used to derive the serve's `completion_key`
    /// (`completion_key_for_interest(sub_key, shape)`). `iter_active` drops it
    /// because most callers only need the interest; the K3 truncated-serve read
    /// path (#1380) needs it to recover each interest's `completion_key` so it
    /// can ask "is THIS interest's cursor-less serve currently truncated?"
    /// without conflating two interests that share an Etag/Ptag shape but differ
    /// only by `SubKey`.
    #[must_use]
    pub fn iter_active_with_keys(&self) -> Vec<(SubKey, LogicalInterest)> {
        self.slots
            .iter()
            .map(|((_, key), slot)| (*key, slot.interest.clone()))
            .collect()
    }

    /// Count of registered `(scope, key)` slots.
    #[allow(dead_code)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    // ─── Legacy bridge helpers ───────────────────────────────────────────────

    /// The `SubKey` minted for interests registered via the legacy `push`
    /// path (`InterestId` → `SubKey` bridge).
    ///
    /// `pub(crate)` (ADR-0045 E1 review item 8): the cache-serve completion
    /// key for `push`-registered follow-feed interests must be derived from
    /// the SAME key the registry mints — a hand-copied derivation at the
    /// serve site would silently diverge if this ever changes (single source
    /// of truth; the R2.1 single-mechanism correction cuts both ways).
    pub(crate) fn legacy_key(id: &InterestId) -> SubKey {
        SubKey::builder("legacy-interest-id").with(id.0).finish()
    }

    fn legacy_scope(interest: &LogicalInterest) -> SubScope {
        use crate::planner::InterestScope;
        match &interest.scope {
            InterestScope::Account(pk) => SubScope::Account(pk.clone()),
            // `ActiveAccount` resolves to a concrete pubkey only at compile
            // time; in the registry it shares the global slot space (it is
            // not isolated per-account until M8 resolves the active pubkey).
            InterestScope::ActiveAccount | InterestScope::Global => SubScope::Global,
        }
    }

    fn legacy_identity(interest: &LogicalInterest) -> SubIdentity {
        SubIdentity::new(
            SubOwnerKey::new("legacy-single-owner"),
            Self::legacy_key(&interest.id),
            Self::legacy_scope(interest),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::{InterestLifecycle, InterestScope, InterestShape};

    fn fixture(id: u64) -> LogicalInterest {
        LogicalInterest {
            id: InterestId(id),
            scope: InterestScope::Global,
            shape: InterestShape::default(),
            hints: Vec::new(),
            lifecycle: InterestLifecycle::Tailing,
            is_indexer_discovery: false,
        }
    }

    fn scoped_fixture(id: u64, scope: InterestScope) -> LogicalInterest {
        LogicalInterest {
            scope,
            ..fixture(id)
        }
    }

    // ── Legacy surface (unchanged behaviour) ─────────────────────────────────

    #[test]
    fn push_then_iter_active_returns_inserted() {
        let mut r = InterestRegistry::new();
        r.push(fixture(1));
        r.push(fixture(2));
        let active = r.iter_active();
        assert_eq!(active.len(), 2);
        // Deterministic order preserved (ids 1,2 keyed → stable slot order).
        let ids: std::collections::BTreeSet<u64> = active.iter().map(|i| i.id.0).collect();
        assert_eq!(ids, [1, 2].into_iter().collect());
    }

    #[test]
    fn push_with_same_id_replaces() {
        let mut r = InterestRegistry::new();
        r.push(fixture(1));
        let mut updated = fixture(1);
        updated.lifecycle = InterestLifecycle::OneShot;
        r.push(updated);
        assert_eq!(r.len(), 1);
        assert!(matches!(
            r.iter_active()[0].lifecycle,
            InterestLifecycle::OneShot,
        ));
    }

    #[test]
    fn withdraw_removes() {
        let mut r = InterestRegistry::new();
        r.push(fixture(1));
        r.withdraw(&InterestId(1));
        assert!(r.is_empty());
    }

    // ── (owner, key, scope) triple ───────────────────────────────────────────

    #[test]
    fn ensure_is_idempotent_does_not_clobber_filter() {
        let mut r = InterestRegistry::new();
        let key = SubKey::new("profile:alice");
        let id1 = SubIdentity::new(SubOwnerKey::new("avatar-A"), key, SubScope::Global);

        let mut first = fixture(1);
        first.lifecycle = InterestLifecycle::Tailing;
        assert!(r.ensure_sub(id1.clone(), first), "first ensure installs");

        // Re-mount: same (scope,key), different/replacement interest. ensure
        // must NOT clobber the existing filter (§3.3 bug class).
        let mut clobber = fixture(1);
        clobber.lifecycle = InterestLifecycle::OneShot;
        let id2 = SubIdentity::new(SubOwnerKey::new("avatar-A"), key, SubScope::Global);
        assert!(
            !r.ensure_sub(id2, clobber),
            "second ensure is a no-op install"
        );

        assert_eq!(r.len(), 1);
        assert!(
            matches!(r.iter_active()[0].lifecycle, InterestLifecycle::Tailing),
            "ensure preserved the original filter"
        );
    }

    #[test]
    fn set_replaces_the_interest() {
        let mut r = InterestRegistry::new();
        let key = SubKey::new("search:foo");
        let id = SubIdentity::new(SubOwnerKey::new("search-view"), key, SubScope::Global);

        let mut v1 = fixture(1);
        v1.lifecycle = InterestLifecycle::Tailing;
        r.set_sub(id.clone(), v1);

        let mut v2 = fixture(1);
        v2.lifecycle = InterestLifecycle::OneShot;
        r.set_sub(id, v2);

        assert_eq!(r.len(), 1);
        assert!(matches!(
            r.iter_active()[0].lifecycle,
            InterestLifecycle::OneShot
        ));
    }

    #[test]
    fn account_scoped_and_global_scoped_are_isolated() {
        let mut r = InterestRegistry::new();
        let key = SubKey::new("profile:alice");

        let acct = SubIdentity::new(
            SubOwnerKey::new("v1"),
            key,
            SubScope::Account("alice".into()),
        );
        let glob = SubIdentity::new(SubOwnerKey::new("v1"), key, SubScope::Global);

        assert!(r.ensure_sub(
            acct,
            scoped_fixture(1, InterestScope::Account("alice".into()))
        ));
        assert!(r.ensure_sub(glob, scoped_fixture(2, InterestScope::Global)));

        // Same SubKey, different scope → two distinct entries.
        assert_eq!(r.len(), 2);
        assert_eq!(r.owner_count(&SubScope::Account("alice".into()), &key), 1);
        assert_eq!(r.owner_count(&SubScope::Global, &key), 1);
    }

    #[test]
    fn dedup_across_owners_keeps_one_interest_refcounted() {
        let mut r = InterestRegistry::new();
        let key = SubKey::new("profile:alice");
        let scope = SubScope::Global;

        let o1 = SubIdentity::new(SubOwnerKey::new("avatar-A"), key, scope.clone());
        let o2 = SubIdentity::new(SubOwnerKey::new("avatar-B"), key, scope.clone());

        assert!(r.ensure_sub(o1.clone(), fixture(1)));
        assert!(
            !r.ensure_sub(o2.clone(), fixture(1)),
            "second owner attaches, does not re-install"
        );

        // Dedup: one logical interest despite two owners.
        assert_eq!(r.iter_active().len(), 1);
        assert_eq!(r.owner_count(&scope, &key), 2);

        // First owner leaves: interest stays (still one owner).
        assert!(!r.drop_owner(&o1));
        assert_eq!(r.iter_active().len(), 1);
        assert_eq!(r.owner_count(&scope, &key), 1);

        // Last owner leaves: interest is dropped.
        assert!(r.drop_owner(&o2));
        assert!(r.is_empty());
    }

    #[test]
    fn drop_owner_on_absent_slot_is_noop() {
        let mut r = InterestRegistry::new();
        let id = SubIdentity::new(
            SubOwnerKey::new("ghost"),
            SubKey::new("nope"),
            SubScope::Global,
        );
        assert!(!r.drop_owner(&id));
        assert!(r.is_empty());
    }
}
