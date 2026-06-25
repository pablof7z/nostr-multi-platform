//! Kernel-owned replacement of a source owner's complete derived interest set.

use std::collections::{BTreeMap, BTreeSet};

use super::Kernel;
use crate::kernel::cache_serve::{InterestRegistration, InterestWrite};
use crate::planner::{InterestLifecycle, InterestScope, LogicalInterest};
use crate::subs::{CompileTrigger, InvalidateReason, SubIdentity, SubKey, SubOwnerKey, SubScope};

/// One child interest produced by reducing another source.
#[derive(Clone, Debug)]
pub struct DependentInterestChild {
    pub key: SubKey,
    pub scope: SubScope,
    pub interest: LogicalInterest,
}

impl DependentInterestChild {
    /// Build a tailing child interest from a typed shape.
    ///
    /// Active-account and global scopes intentionally share the same SubKey
    /// derivation as `open_interest`, so a dependent child and an explicit
    /// `OpenInterest` for the same shape/scope dedup onto one live slot.
    #[must_use]
    pub fn tailing(shape: nmp_planner::InterestShape, scope: nmp_planner::InterestScope) -> Self {
        let sub_scope = match &scope {
            InterestScope::Account(pubkey) => SubScope::Account(pubkey.clone()),
            InterestScope::ActiveAccount | InterestScope::Global => SubScope::Global,
        };
        let key = SubKey::builder("open-interest")
            .with(&shape)
            .with(scope_key_part(&scope))
            .finish();
        let interest = LogicalInterest {
            scope,
            shape,
            lifecycle: InterestLifecycle::Tailing,
            ..LogicalInterest::default()
        };
        Self {
            key,
            scope: sub_scope,
            interest,
        }
    }

    fn identity(&self, owner: SubOwnerKey) -> SubIdentity {
        SubIdentity::new(owner, self.key, self.scope.clone())
    }
}

fn scope_key_part(scope: &InterestScope) -> u32 {
    match scope {
        InterestScope::ActiveAccount => 0,
        InterestScope::Global => 1,
        InterestScope::Account(_) => 2,
    }
}

/// Diagnostics returned by one replacement pass.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct DependentInterestSetOutcome {
    pub registered_children: usize,
    pub withdrawn_children: usize,
    pub closed_slots: usize,
    pub changed_registrations: usize,
}

impl Kernel {
    /// Replace every child interest owned by `owner`.
    ///
    /// The caller supplies the freshly-reduced set. The kernel withdraws child
    /// identities that disappeared, upserts current children through the same
    /// sealed `register_interest` machinery as `open_interest`, and emits at
    /// most one compile invalidation for the whole replacement.
    pub(crate) fn replace_dependent_interest_set(
        &mut self,
        owner: SubOwnerKey,
        children: Vec<DependentInterestChild>,
        reason: &str,
    ) -> DependentInterestSetOutcome {
        let mut next = BTreeMap::<SubIdentity, LogicalInterest>::new();
        for child in children {
            next.insert(child.identity(owner), child.interest);
        }
        let next_identities = next.keys().cloned().collect::<BTreeSet<_>>();
        let previous: BTreeMap<SubIdentity, LogicalInterest> = self
            .dependent_interest_sets
            .get(&owner)
            .cloned()
            .unwrap_or_default();

        let mut outcome = DependentInterestSetOutcome {
            registered_children: next_identities.len(),
            ..Default::default()
        };

        for identity in previous
            .keys()
            .filter(|identity| !next_identities.contains(*identity))
        {
            outcome.withdrawn_children += 1;
            if self.lifecycle.registry_mut().drop_owner(identity) {
                outcome.closed_slots += 1;
                if let Some(old) = previous.get(identity) {
                    self.cancel_pending_interest_cache_serve(&identity.key, &old.shape);
                }
            }
        }

        let registrations = next
            .iter()
            .map(|(identity, interest)| InterestRegistration {
                identity: identity.clone(),
                interest: interest.clone(),
                policy: InterestWrite::Replace,
            })
            .collect::<Vec<_>>();
        let outcomes = self.apply_interest_registrations(&registrations);
        outcome.changed_registrations = outcomes
            .iter()
            .filter(|registration| registration.changed)
            .count();

        if outcome.changed_registrations > 0 {
            self.run_cache_serve_step();
        }
        if outcome.closed_slots > 0 || outcome.changed_registrations > 0 {
            self.lifecycle
                .enqueue_trigger(CompileTrigger::InvalidateCompile {
                    reason: InvalidateReason::External(reason.to_string()),
                });
        }
        if next.is_empty() {
            self.dependent_interest_sets.remove(&owner);
        } else {
            self.dependent_interest_sets.insert(owner, next);
        }

        outcome
    }
}
