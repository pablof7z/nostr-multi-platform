//! `CustomPerspectiveId` RESOLUTION (#1740 step 4).
//!
//! Step 3 left every `Custom` reference fail-closed; this module resolves a
//! `Custom` reference back to its REGISTERED [`nmp_feed::CustomPerspectiveDef`]
//! (a CLOSED `FeedScope` + ranking — pure data, registered out-of-band by the
//! host runtime) and compiles it through the SAME step-3
//! resolver. There is NO second resolver and NO closure crosses the boundary:
//! the registry stores data, this module looks it up and calls
//! [`super::resolve::resolve_scope`].
//!
//! Three reference points, all keyed by the same registry look-up:
//! * `FeedScope::CustomPerspectiveId(id)` (acquisition) → resolve the registered
//!   scope as the acquisition source.
//! * `FeedAdmission::Custom(id)` (admission gate) → AND the registered scope's
//!   compiled admission predicate ON TOP of the acquisition's — the admission
//!   semantics of `Intersection(acquisition, gate)`. The gate's DEPENDENCY +
//!   row acquisition is KEPT (so its predicate goes live on a cold open); the
//!   AND keeps the result faithful (gate-only rows are filtered out).
//! * `FeedRanking::Custom(id)` (order) → use the registered ranking, which must
//!   itself be engine-honorable or the open fails closed.
//!
//! Fail-closed (D6) at every step: an UNREGISTERED id has no definition →
//! [`FeedOpenError::ScopeNotSupportedYet`], and any already-registered resolver
//! observers from a partially-resolved composite are revoked so nothing leaks
//! (D8). A registered-but-unhonorable ranking also fails closed (never silently
//! mis-orders).

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::{FeedOpenError, FeedSessionHost};
use nmp_core::substrate::KernelEvent;
use nmp_feed::{CustomPerspectiveId, FeedRanking, FeedScope, RootAdmission};
use nmp_planner::InterestShape;

use super::resolve::resolve_scope;
use super::source::{ExtraAcquisition, LiveShape, ReducedSource};

fn not_supported(scope: &'static str) -> FeedOpenError {
    FeedOpenError::ScopeNotSupportedYet { scope }
}

/// Validate the requested ranking compiles to an engine-honorable order.
///
/// The session engine sorts roots newest-first (`ChronologicalDesc`) ONLY.
/// `ChronologicalAsc` is not wired. `Custom(id)` resolves to the registered
/// perspective's ranking, which is itself validated the same way — an
/// unregistered id, or a registered ranking the engine cannot honor, fails
/// closed rather than silently mis-ordering (D6).
pub(super) fn resolve_ranking(
    app: &impl FeedSessionHost,
    ranking: &FeedRanking,
) -> Result<(), FeedOpenError> {
    match ranking {
        FeedRanking::ChronologicalDesc => Ok(()),
        FeedRanking::ChronologicalAsc => Err(not_supported("custom-ranking")),
        FeedRanking::Custom(id) => {
            let def = app
                .custom_perspective(id)
                .ok_or_else(|| not_supported("custom-ranking"))?;
            // The registered ranking must itself be engine-honorable. A custom id
            // is NOT allowed to nest another `Custom` ranking (that would be an
            // unbounded indirection with no concrete order) — only the two
            // concrete chronological orders resolve, and only `Desc` is wired.
            match def.ranking {
                FeedRanking::ChronologicalDesc => Ok(()),
                FeedRanking::ChronologicalAsc | FeedRanking::Custom(_) => {
                    Err(not_supported("custom-ranking"))
                }
            }
        }
    }
}

/// Resolve the acquisition scope, expanding a `CustomPerspectiveId` to its
/// registered definition's scope.
///
/// A non-custom scope delegates straight to [`resolve_scope`] (the step-3
/// compiler). A `CustomPerspectiveId(id)` looks the id up; an unregistered id
/// fails closed. The registered scope is then resolved through the SAME step-3
/// compiler — so set algebra (e.g. a registered `Intersection(Tag, ContactList)`)
/// composes exactly as a directly-declared scope would.
pub(super) fn resolve_acquisition(
    app: &impl FeedSessionHost,
    scope: &FeedScope,
    kinds: &BTreeSet<u32>,
) -> Result<ReducedSource, FeedOpenError> {
    match scope {
        FeedScope::CustomPerspectiveId(id) => {
            let def = app
                .custom_perspective(id)
                .ok_or_else(|| not_supported("CustomPerspectiveId"))?;
            // A registered acquisition must NOT itself be a custom id (no
            // unbounded indirection) — resolve its concrete scope directly. A
            // registered `ActiveUserFollows` / nested `CustomPerspectiveId` is not
            // a session-engine scope → `resolve_scope` fails it closed.
            resolve_scope(app, &def.acquisition, kinds)
        }
        other => resolve_scope(app, other, kinds),
    }
}

/// Intersect the registered admission perspective's compiled predicate onto the
/// already-resolved acquisition.
///
/// `FeedAdmission::Custom(id)` narrows which acquired roots render to those the
/// gate ALSO admits — exactly the admission semantics of
/// `Intersection(acquisition, gate)`. So we resolve the registered perspective's
/// scope through the SAME step-3 compiler and combine it with the acquisition
/// the way [`super::set_algebra`] combines an `Intersection`:
///
/// * admission: AND the two LIVE, EVENT-AWARE predicates (admit iff BOTH admit);
/// * acquisition: KEEP the gate's interests / live shape / extra acquisition.
///
/// Keeping the gate's acquisition is REQUIRED, not over-acquisition: a
/// `ListMembers` / `Wot` gate's predicate is empty until its DEPENDENCY events
/// (the kind:30000 list, the seed's kind:3 + its follows' kind:3) are fetched —
/// those dependency fetches ride on the gate's `interests` / `extra_acquisition`.
/// Dropping them would leave the gate predicate admitting NOBODY on a cold open
/// (a silent fail-closed-wrong feed that depends on some other session ambiently
/// ingesting the list). The gate's member-ROW timeline is also acquired, exactly
/// as `Intersection` does — those rows render only if the acquisition predicate
/// also admits them, so the AND keeps the result faithful (no row leaks in).
///
/// An unregistered id fails closed; the acquisition's already-registered resolver
/// observers are revoked first so nothing leaks (D8).
pub(super) fn apply_custom_admission(
    app: &impl FeedSessionHost,
    acquisition: ReducedSource,
    id: &CustomPerspectiveId,
    kinds: &BTreeSet<u32>,
) -> Result<ReducedSource, FeedOpenError> {
    let Some(def) = app.custom_perspective(id) else {
        // Unregistered → fail closed, revoking the acquisition's observers so the
        // partially-resolved open leaks nothing.
        revoke_resolved(app, acquisition);
        return Err(not_supported("custom-admission"));
    };

    // Resolve the admission perspective's scope through the SAME step-3 compiler.
    // If it fails, revoke the acquisition's observers too (fail closed, no leak).
    let gate = match resolve_scope(app, &def.acquisition, kinds) {
        Ok(gate) => gate,
        Err(e) => {
            revoke_resolved(app, acquisition);
            return Err(e);
        }
    };

    Ok(combine_admission_gate(acquisition, gate))
}

/// Combine an acquisition's resolved scope with a gate's resolved scope as an
/// admission INTERSECTION (the pure core of [`apply_custom_admission`], factored
/// out so it is unit-testable without a runtime host).
///
/// * admission: AND the two LIVE predicates (admit iff BOTH admit);
/// * acquisition: KEEP the gate's interests/live_shape/extra so its predicate
///   goes live (its dependency events get fetched);
/// * reactivity/teardown: both sides' reset hooks + observers flow up.
pub(super) fn combine_admission_gate(
    acquisition: ReducedSource,
    gate: ReducedSource,
) -> ReducedSource {
    let ReducedSource {
        op_session_identity: acq_op_session_identity,
        admission: acq_admission,
        attribution: acq_attribution,
        mut interests,
        live_shape: acq_live_shape,
        live_shapes: acq_live_shapes,
        observer_scope: acq_observer_scope,
        extra_acquisition: acq_extra,
        mut reset_hooks,
        mut source_effect_hooks,
        mut resolver_observer_ids,
        mut identity_observer_ids,
        mut resolver_teardown,
        active_follow_set,
    } = acquisition;
    let op_session_identity = acq_op_session_identity.combine(gate.op_session_identity);

    // AND the two LIVE, EVENT-AWARE predicates: a root renders iff the
    // acquisition admits it AND the custom admission perspective admits it.
    let combined: RootAdmission = {
        let lp = acq_admission;
        let rp = gate.admission;
        Arc::new(move |ev: &KernelEvent| lp(ev) && rp(ev))
    };
    let combined_attribution: nmp_feed::FollowPredicate = {
        let lp = acq_attribution;
        let rp = gate.attribution;
        Arc::new(move |pubkey: &str| lp(pubkey) && rp(pubkey))
    };

    // Acquisition combine — KEEP the gate's dependency + row acquisition so its
    // predicate goes live (Intersection discipline, mirroring `set_algebra`).
    interests.extend(gate.interests);
    let live_shape: LiveShape = {
        let ls = acq_live_shape;
        let rs = gate.live_shape;
        Arc::new(move || merge_live_shapes(&ls, &rs))
    };
    let live_shapes = {
        let ls = acq_live_shapes;
        let rs = gate.live_shapes;
        Arc::new(move || {
            let mut shapes = ls();
            shapes.extend(rs());
            shapes
        })
    };
    let observer_scope = combine_observer_scope(&acq_observer_scope, &gate.observer_scope);
    let extra_acquisition: ExtraAcquisition = {
        let le = acq_extra;
        let re = gate.extra_acquisition;
        Arc::new(move || {
            let mut shapes = le();
            shapes.extend(re());
            shapes
        })
    };

    // Both sides stay reactive + are torn down (the gate must track changes so
    // its exclusion follows the live list/graph).
    reset_hooks.extend(gate.reset_hooks);
    source_effect_hooks.extend(gate.source_effect_hooks);
    resolver_observer_ids.extend(gate.resolver_observer_ids);
    identity_observer_ids.extend(gate.identity_observer_ids);
    resolver_teardown.extend(gate.resolver_teardown);

    ReducedSource {
        op_session_identity,
        admission: combined,
        attribution: combined_attribution,
        interests,
        live_shape,
        live_shapes,
        observer_scope,
        extra_acquisition,
        reset_hooks,
        source_effect_hooks,
        resolver_observer_ids,
        identity_observer_ids,
        resolver_teardown,
        active_follow_set,
    }
}

/// Union two live acquisition shapes (authors + kinds + tags). A `None` child
/// contributes nothing; both `None` ⇒ `None`. Mirrors `set_algebra::merge_shapes`
/// for the Intersection case (both sides acquired).
fn merge_live_shapes(left: &LiveShape, right: &LiveShape) -> Option<InterestShape> {
    match (left(), right()) {
        (None, None) => None,
        (Some(shape), None) | (None, Some(shape)) => Some(shape),
        (Some(mut a), Some(b)) => {
            a.authors.extend(b.authors);
            a.kinds.extend(b.kinds);
            a.event_ids.extend(b.event_ids);
            a.addresses.extend(b.addresses);
            for (key, vals) in b.tags {
                a.tags.entry(key).or_default().extend(vals);
            }
            Some(a)
        }
    }
}

fn combine_observer_scope(
    left: &nmp_planner::InterestScope,
    right: &nmp_planner::InterestScope,
) -> nmp_planner::InterestScope {
    if matches!(left, nmp_planner::InterestScope::Global)
        || matches!(right, nmp_planner::InterestScope::Global)
    {
        nmp_planner::InterestScope::Global
    } else {
        nmp_planner::InterestScope::ActiveAccount
    }
}

/// Revoke every resolver observer a resolved scope registered (fail-closed
/// cleanup when a later resolution step errors — no leak, D8).
fn revoke_observers(app: &impl FeedSessionHost, resolved: &ReducedSource) {
    for id in &resolved.resolver_observer_ids {
        app.observed_projection_handle().close(*id);
    }
    for id in &resolved.identity_observer_ids {
        (app.unregister_identity_change_observer_action(*id))();
    }
}

fn revoke_resolved(app: &impl FeedSessionHost, resolved: ReducedSource) {
    revoke_observers(app, &resolved);
    for teardown in resolved.resolver_teardown {
        teardown();
    }
}

#[cfg(test)]
mod tests {
    //! Row-level oracle for the custom-admission combine — over the REAL
    //! production [`combine_admission_gate`], NOT a test-side `AdmitExpr::And`
    //! mirror. Proves the combined admission is a faithful AND (member-in,
    //! non-member-out, would FAIL if the gate were dropped or OR'd) and that the
    //! gate's DEPENDENCY acquisition is preserved (so a `ListMembers`/`Wot` gate
    //! predicate can go live on a cold open).

    use super::*;
    use crate::source::AcquisitionInterest;
    use crate::source::OpSessionIdentity;
    use nmp_core::substrate::{EventId, KernelEvent};
    use nmp_feed::AdmitExpr;
    use nmp_planner::{InterestScope, InterestShape};

    const ACQ_MEMBER: &str = "acac000000000000000000000000000000000000000000000000000000000001";
    const GATE_MEMBER: &str = "9a7e000000000000000000000000000000000000000000000000000000000001";
    const BOTH: &str = "b07f000000000000000000000000000000000000000000000000000000000001";
    const STRANGER: &str = "57a9000000000000000000000000000000000000000000000000000000000001";

    fn note(author: &str) -> KernelEvent {
        KernelEvent {
            id: EventId::from("2".repeat(64)),
            author: author.to_string(),
            kind: 1,
            created_at: 100,
            tags: Vec::new(),
            content: String::new(),
            relay_provenance: Vec::new(),
        }
    }

    /// A resolved scope admitting exactly `authors`, carrying the given fixed
    /// acquisition interest (so we can assert the gate's acquisition survives).
    fn scope(authors: &[&str], interest: Option<AcquisitionInterest>) -> ReducedSource {
        let admission = AdmitExpr::Authors(authors.iter().map(|s| (*s).to_string()).collect())
            .to_root_admission();
        let author_set: std::collections::BTreeSet<String> =
            authors.iter().map(|s| (*s).to_string()).collect();
        ReducedSource {
            op_session_identity: OpSessionIdentity::RequireActive,
            admission,
            attribution: Arc::new(move |pubkey: &str| author_set.contains(pubkey)),
            interests: interest.into_iter().collect(),
            live_shape: Arc::new(|| None),
            live_shapes: Arc::new(Vec::new),
            observer_scope: InterestScope::ActiveAccount,
            extra_acquisition: Arc::new(Vec::new),
            reset_hooks: Vec::new(),
            source_effect_hooks: Vec::new(),
            resolver_observer_ids: Vec::new(),
            identity_observer_ids: Vec::new(),
            resolver_teardown: Vec::new(),
            active_follow_set: None,
        }
    }

    fn kind_interest(kind: u32, scope: InterestScope) -> AcquisitionInterest {
        AcquisitionInterest {
            shape: InterestShape {
                kinds: [kind].into_iter().collect(),
                ..InterestShape::default()
            },
            scope,
        }
    }

    #[test]
    fn combine_is_a_faithful_and_member_in_non_member_out() {
        // acquisition admits {ACQ_MEMBER, BOTH}; gate admits {GATE_MEMBER, BOTH}.
        let acquisition = scope(&[ACQ_MEMBER, BOTH], None);
        let gate = scope(&[GATE_MEMBER, BOTH], None);
        let combined = combine_admission_gate(acquisition, gate);
        let admit = &combined.admission;

        // Only the author in BOTH sets renders.
        assert!(admit(&note(BOTH)), "in acquisition AND gate → admitted");
        // In acquisition but NOT the gate → excluded. This is the assertion that
        // would FAIL if the gate were dropped (the step-3-only behavior) or OR'd.
        assert!(
            !admit(&note(ACQ_MEMBER)),
            "acquired but the gate excludes it → NOT admitted"
        );
        // In the gate but NOT acquired → excluded (gate is a filter, not a source).
        assert!(
            !admit(&note(GATE_MEMBER)),
            "gate would admit but acquisition does not → NOT admitted"
        );
        assert!(!admit(&note(STRANGER)), "in neither set → NOT admitted");
    }

    #[test]
    fn combine_preserves_gate_dependency_acquisition() {
        // The gate carries a dependency interest (e.g. the kind:30000 list event
        // a `ListMembers` gate must fetch to populate its predicate). The combine
        // MUST keep it — dropping it would leave the gate admitting nobody on a
        // cold open. Both sides' interests survive.
        let acquisition = scope(&[ACQ_MEMBER], Some(kind_interest(1, InterestScope::Global)));
        let gate = scope(
            &[GATE_MEMBER],
            Some(kind_interest(30_000, InterestScope::ActiveAccount)),
        );
        let combined = combine_admission_gate(acquisition, gate);

        assert!(
            combined
                .interests
                .iter()
                .any(|interest| interest.shape.kinds == [1].into_iter().collect()
                    && interest.scope == InterestScope::Global),
            "acquisition interest preserved"
        );
        assert!(
            combined
                .interests
                .iter()
                .any(
                    |interest| interest.shape.kinds == [30_000].into_iter().collect()
                        && interest.scope == InterestScope::ActiveAccount
                ),
            "gate DEPENDENCY interest preserved (predicate can go live on a cold open)"
        );
    }
}
