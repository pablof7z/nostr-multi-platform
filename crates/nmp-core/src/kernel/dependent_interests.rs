//! Kernel-owned replacement of a source owner's complete derived interest set.

use std::collections::{BTreeMap, BTreeSet};

use super::Kernel;
use crate::kernel::cache_serve::{InterestRegistration, InterestWrite};
use crate::planner::{InterestLifecycle, InterestScope, LogicalInterest};
use crate::subs::{CompileTrigger, InvalidateReason, SubIdentity, SubKey, SubOwnerKey, SubScope};

/// One child interest produced by reducing another source.
#[derive(Clone, Debug, Eq, PartialEq)]
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

/// Ordered dependent-interest mutation produced by a private reconciler.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependentInterestDelta {
    pub commands: Vec<DependentInterestDeltaCommand>,
}

impl DependentInterestDelta {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

/// One precise mutation for a dependent-interest owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DependentInterestDeltaCommand {
    Open(DependentInterestChild),
    Replace(DependentInterestChild),
    Refresh(DependentInterestChild),
    Close(DependentInterestChild),
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
    #[allow(dead_code)] // Retained for the full-set command/API while Trellis callers use deltas.
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

    /// Apply an ordered dependent-interest delta for one source owner.
    ///
    /// This is the authoritative path for private reconcilers such as Trellis:
    /// callers provide exact open/replace/close commands, while the kernel
    /// keeps the lifecycle registry, cache-serve, and compile invalidation
    /// mechanics centralized.
    pub(crate) fn apply_dependent_interest_delta(
        &mut self,
        owner: SubOwnerKey,
        delta: DependentInterestDelta,
        reason: &str,
    ) -> DependentInterestSetOutcome {
        let mut outcome = DependentInterestSetOutcome::default();
        let mut upserts = Vec::new();
        for command in delta.commands {
            match command {
                DependentInterestDeltaCommand::Open(child)
                | DependentInterestDeltaCommand::Replace(child)
                | DependentInterestDeltaCommand::Refresh(child) => {
                    upserts.push(child);
                }
                DependentInterestDeltaCommand::Close(child) => {
                    outcome.withdrawn_children += 1;
                    let identity = child.identity(owner);
                    if self.lifecycle.registry_mut().drop_owner(&identity) {
                        outcome.closed_slots += 1;
                        self.cancel_pending_interest_cache_serve(
                            &identity.key,
                            &child.interest.shape,
                        );
                    }
                }
            }
        }

        for child in upserts {
            outcome.registered_children += 1;
            let registration = InterestRegistration {
                identity: child.identity(owner),
                interest: child.interest,
                policy: InterestWrite::Replace,
            };
            let registration_outcomes = self.apply_interest_registrations(&[registration]);
            if registration_outcomes[0].changed {
                outcome.changed_registrations += 1;
            }
        }

        if outcome.changed_registrations > 0 {
            self.run_cache_serve_step();
        }
        if outcome.closed_slots > 0 || outcome.changed_registrations > 0 {
            self.lifecycle
                .enqueue_trigger(CompileTrigger::InvalidateCompile {
                    reason: InvalidateReason::External(reason.to_string()),
                });
        }

        outcome
    }
}
