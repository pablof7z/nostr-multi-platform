//! Logical-interest registry — the single writer of the active-interest set (D4).
//!
//! View modules and action modules register `LogicalInterest`s via
//! [`crate::kernel::Kernel::register_interest`] (the single front-door); the
//! planner reads via [`InterestRegistry::iter_active`]. The registry is keyed
//! by the
//! `(owner, key, scope)` triple from `docs/design/nostrdb-notedeck-lessons.md`
//! §3.2 (see [`crate::subs::sub_key`]):
//!
//! - **Dedup across owners.** Many owners may attach to the same
//!   `(scope, key)`; the registry keeps *one* live [`LogicalInterest`] per
//!   `(scope, key)` and refcounts owners. The interest stays alive while any
//!   owner is attached and is dropped when the last owner leaves.
//! - **`EnsureAbsent` vs `Replace`** (§3.3). [`InterestRegistry::apply`] with
//!   [`crate::kernel::cache_serve::InterestWrite::EnsureAbsent`] is idempotent
//!   register-if-absent: it attaches the owner and, only if the `(scope, key)` is
//!   *absent*, installs the interest — a re-mount never clobbers an existing
//!   filter. [`crate::kernel::cache_serve::InterestWrite::Replace`] is upsert:
//!   it attaches the owner and *replaces* the interest for `(scope, key)`.
//! - **Account vs global isolation** (§3.4). The same [`SubKey`] under
//!   `SubScope::Account(pubkey)` and `SubScope::Global` are distinct entries.
//!
//! D4: this is the authoritative active set; the planner reads via
//! [`InterestRegistry::iter_active`] but never mutates. Snapshots are
//! deterministically ordered by `(scope, key)` so plan-ids stay stable
//! across recompilations (D8 — no reactivity regression).
//!
//! ## Sealing
//!
//! All registry mutations go through [`InterestRegistry::apply`], which requires
//! a `&RegistryWriteToken`. The token is minted exclusively inside
//! `crate::kernel::cache_serve` (the front-door). Production code outside
//! `cache_serve` cannot mutate the interest set. A `for_test()` seam is
//! available under `#[cfg(any(test, feature = "test-support"))]` so
//! registry-level unit tests (which hold no `Kernel`) can still call `apply`.

use std::collections::BTreeMap;

use crate::kernel::cache_serve::{InterestWrite, RegistrationOutcome, RegistryWriteToken};
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
///
/// All mutations go through [`InterestRegistry::apply`] which requires a
/// [`RegistryWriteToken`] — only `crate::kernel::cache_serve` can mint one
/// in production code.
#[derive(Default)]
pub struct InterestRegistry {
    /// Live interests keyed by the shared `(scope, key)` pair. `BTreeMap`
    /// keeps the snapshot deterministically ordered (D8).
    slots: BTreeMap<(SubScope, SubKey), Slot>,
}

/// True iff the stored interest differs from the incoming one in a field the
/// planner compiles on. Checks shape, lifecycle (OneShot↔Tailing wire upgrade),
/// hints (claim-expansion W7 REQs), and indexer-discovery flag.
/// `since`/`until`/`limit` are excluded (watermark+relay refinement owns them,
/// matching `completion_key_for_interest`'s exclusions).
fn plan_relevant_change(stored: &LogicalInterest, incoming: &LogicalInterest) -> bool {
    stored.shape != incoming.shape
        || stored.lifecycle != incoming.lifecycle
        || stored.hints != incoming.hints
        || stored.is_indexer_discovery != incoming.is_indexer_discovery
}

impl InterestRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    // ─── Token-gated write surface ────────────────────────────────────────────

    /// The single token-gated mutator. Only code that holds a
    /// [`RegistryWriteToken`] (minted exclusively inside
    /// `crate::kernel::cache_serve`) can call this.
    ///
    /// `EnsureAbsent`: attach owner; install only if absent (never clobbers an
    /// existing filter — the §3.3 bug class). Returns `{newly_installed,
    /// changed}` where both equal the "newly installed" predicate.
    ///
    /// `Replace`: attach owner; unconditionally replace the interest. Returns
    /// `changed = true` when newly installed OR any plan-relevant field differed
    /// (shape, lifecycle, hints, is_indexer_discovery).
    #[must_use]
    pub(crate) fn apply(
        &mut self,
        _t: &RegistryWriteToken,
        policy: InterestWrite,
        identity: SubIdentity,
        interest: LogicalInterest,
    ) -> RegistrationOutcome {
        let shared = identity.shared();
        match policy {
            InterestWrite::EnsureAbsent => {
                if let Some(slot) = self.slots.get_mut(&shared) {
                    slot.owners.insert(identity.owner);
                    RegistrationOutcome {
                        newly_installed: false,
                        changed: false,
                    }
                } else {
                    let mut owners = std::collections::BTreeSet::new();
                    owners.insert(identity.owner);
                    self.slots.insert(shared, Slot { interest, owners });
                    RegistrationOutcome {
                        newly_installed: true,
                        changed: true,
                    }
                }
            }
            InterestWrite::Replace => {
                if let Some(slot) = self.slots.get_mut(&shared) {
                    let changed = plan_relevant_change(&slot.interest, &interest);
                    slot.owners.insert(identity.owner);
                    slot.interest = interest;
                    RegistrationOutcome {
                        newly_installed: false,
                        changed,
                    }
                } else {
                    let mut owners = std::collections::BTreeSet::new();
                    owners.insert(identity.owner);
                    self.slots.insert(shared, Slot { interest, owners });
                    RegistrationOutcome {
                        newly_installed: true,
                        changed: true,
                    }
                }
            }
        }
    }

    // ─── Un-registration (not sealed — removing a sub is always safe) ─────────

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

    // ─── Read-only surface ────────────────────────────────────────────────────

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
    /// can ask "is THIS interest's serve currently truncated at the budget?"
    /// without conflating two interests that share the same single-letter tag
    /// shape (now a time-bounded, since/until-cursored `StoreQuery::Tags`) but
    /// differ only by `SubKey`.
    #[must_use]
    pub fn iter_active_with_keys(&self) -> Vec<(SubKey, LogicalInterest)> {
        self.slots
            .iter()
            .map(|((_, key), slot)| (*key, slot.interest.clone()))
            .collect()
    }

    /// Owner refcount for a `(scope, key)` slot (diagnostics / tests).
    #[must_use]
    pub fn owner_count(&self, scope: &SubScope, key: &SubKey) -> usize {
        self.slots
            .get(&(scope.clone(), *key))
            .map_or(0, |s| s.owners.len())
    }

    /// Owner refcounts keyed by planner interest id.
    ///
    /// The registry remains the single writer for logical-interest ownership;
    /// diagnostics use this read model to join current wire rows back to their
    /// originating planner interests without storing logical state on transport
    /// rows.
    #[must_use]
    pub(crate) fn owner_counts_by_interest_id(&self) -> BTreeMap<InterestId, usize> {
        let mut counts = BTreeMap::new();
        for slot in self.slots.values() {
            let count = counts.entry(slot.interest.id.clone()).or_insert(0usize);
            *count = count.saturating_add(slot.owners.len());
        }
        counts
    }

    /// Count of registered `(scope, key)` slots.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::cache_serve::RegistryWriteToken;
    use crate::planner::{InterestId, InterestLifecycle, InterestScope, InterestShape};

    fn token() -> RegistryWriteToken {
        RegistryWriteToken::for_test()
    }

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

    fn global_identity(key: SubKey) -> SubIdentity {
        SubIdentity::new(SubOwnerKey::new("test-owner"), key, SubScope::Global)
    }

    #[test]
    fn replace_then_iter_active_returns_inserted() {
        let mut r = InterestRegistry::new();
        let t = token();
        let _ = r.apply(
            &t,
            InterestWrite::Replace,
            global_identity(SubKey::new("replace-slot-1")),
            fixture(1),
        );
        let _ = r.apply(
            &t,
            InterestWrite::Replace,
            global_identity(SubKey::new("replace-slot-2")),
            fixture(2),
        );
        let active = r.iter_active();
        assert_eq!(active.len(), 2);
        let ids: std::collections::BTreeSet<u64> = active.iter().map(|i| i.id.0).collect();
        assert_eq!(ids, [1, 2].into_iter().collect());
    }

    #[test]
    fn replace_with_same_id_replaces() {
        let mut r = InterestRegistry::new();
        let t = token();
        let identity = global_identity(SubKey::new("replace-slot"));
        let _ = r.apply(&t, InterestWrite::Replace, identity.clone(), fixture(1));
        let mut updated = fixture(1);
        updated.lifecycle = InterestLifecycle::OneShot;
        let reg = r.apply(&t, InterestWrite::Replace, identity, updated);
        assert_eq!(r.len(), 1);
        assert!(matches!(
            r.iter_active()[0].lifecycle,
            InterestLifecycle::OneShot,
        ));
        // Shape unchanged, lifecycle changed → plan-relevant → changed == true.
        assert!(reg.changed);
    }

    // ── (owner, key, scope) triple ───────────────────────────────────────────

    #[test]
    fn ensure_is_idempotent_does_not_clobber_filter() {
        let mut r = InterestRegistry::new();
        let t = token();
        let key = SubKey::new("profile:alice");
        let id1 = SubIdentity::new(SubOwnerKey::new("avatar-A"), key, SubScope::Global);

        let mut first = fixture(1);
        first.lifecycle = InterestLifecycle::Tailing;
        let r1 = r.apply(&t, InterestWrite::EnsureAbsent, id1.clone(), first);
        assert!(r1.newly_installed, "first ensure installs");

        // Re-mount: same (scope,key), different/replacement interest. ensure
        // must NOT clobber the existing filter (§3.3 bug class).
        let mut clobber = fixture(1);
        clobber.lifecycle = InterestLifecycle::OneShot;
        let id2 = SubIdentity::new(SubOwnerKey::new("avatar-A"), key, SubScope::Global);
        let r2 = r.apply(&t, InterestWrite::EnsureAbsent, id2, clobber);
        assert!(!r2.newly_installed, "second ensure is a no-op install");

        assert_eq!(r.len(), 1);
        assert!(
            matches!(r.iter_active()[0].lifecycle, InterestLifecycle::Tailing),
            "ensure preserved the original filter"
        );
    }

    #[test]
    fn replace_replaces_the_interest() {
        let mut r = InterestRegistry::new();
        let t = token();
        let key = SubKey::new("search:foo");
        let id = SubIdentity::new(SubOwnerKey::new("search-view"), key, SubScope::Global);

        let mut v1 = fixture(1);
        v1.lifecycle = InterestLifecycle::Tailing;
        let _ = r.apply(&t, InterestWrite::Replace, id.clone(), v1);

        let mut v2 = fixture(1);
        v2.lifecycle = InterestLifecycle::OneShot;
        let _ = r.apply(&t, InterestWrite::Replace, id, v2);

        assert_eq!(r.len(), 1);
        assert!(matches!(
            r.iter_active()[0].lifecycle,
            InterestLifecycle::OneShot
        ));
    }

    #[test]
    fn account_scoped_and_global_scoped_are_isolated() {
        let mut r = InterestRegistry::new();
        let t = token();
        let key = SubKey::new("profile:alice");

        let acct = SubIdentity::new(
            SubOwnerKey::new("v1"),
            key,
            SubScope::Account("alice".into()),
        );
        let glob = SubIdentity::new(SubOwnerKey::new("v1"), key, SubScope::Global);

        let _ = r.apply(
            &t,
            InterestWrite::EnsureAbsent,
            acct,
            scoped_fixture(1, InterestScope::Account("alice".into())),
        );
        let _ = r.apply(
            &t,
            InterestWrite::EnsureAbsent,
            glob,
            scoped_fixture(2, InterestScope::Global),
        );

        // Same SubKey, different scope → two distinct entries.
        assert_eq!(r.len(), 2);
        assert_eq!(r.owner_count(&SubScope::Account("alice".into()), &key), 1);
        assert_eq!(r.owner_count(&SubScope::Global, &key), 1);
    }

    #[test]
    fn dedup_across_owners_keeps_one_interest_refcounted() {
        let mut r = InterestRegistry::new();
        let t = token();
        let key = SubKey::new("profile:alice");
        let scope = SubScope::Global;

        let o1 = SubIdentity::new(SubOwnerKey::new("avatar-A"), key, scope.clone());
        let o2 = SubIdentity::new(SubOwnerKey::new("avatar-B"), key, scope.clone());

        let r1 = r.apply(&t, InterestWrite::EnsureAbsent, o1.clone(), fixture(1));
        assert!(r1.newly_installed);
        let r2 = r.apply(&t, InterestWrite::EnsureAbsent, o2.clone(), fixture(1));
        assert!(
            !r2.newly_installed,
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

    #[test]
    fn replace_no_change_returns_changed_false() {
        let mut r = InterestRegistry::new();
        let t = token();
        let key = SubKey::new("profile:alice");
        let id = global_identity(key);
        let interest = fixture(42);

        let _ = r.apply(&t, InterestWrite::Replace, id.clone(), interest.clone());
        // Second Replace with identical interest: shape/lifecycle/hints all same.
        let reg = r.apply(&t, InterestWrite::Replace, id, interest);
        assert!(!reg.changed, "identical replace must not flag changed");
        assert!(!reg.newly_installed);
    }

    #[test]
    fn plan_relevant_change_detects_lifecycle_diff() {
        let base = fixture(1);
        let mut upgraded = fixture(1);
        upgraded.lifecycle = crate::planner::InterestLifecycle::OneShot;
        assert!(
            plan_relevant_change(&base, &upgraded),
            "lifecycle change is plan-relevant"
        );
    }
}
