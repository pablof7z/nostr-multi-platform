//! Set algebra over compiled pubkey-set scopes (#1740 step 3).
//!
//! `Union` / `Intersection` / `Difference` recursively resolve their two child
//! scopes and combine the results:
//!
//! | scope          | admission combine | acquisition combine        |
//! |----------------|-------------------|----------------------------|
//! | `Union`        | `l OR r`          | both children's interests  |
//! | `Intersection` | `l AND r`         | both children's interests  |
//! | `Difference`   | `l AND NOT r`     | LEFT child's interests only |
//!
//! The combined admission is a live predicate over the children's live
//! predicates (so reactive children stay reactive under the combinator). Both
//! children's reset hooks + resolver observer ids flow up, so a change in either
//! underlying set resets the combined window and the close withdraws every
//! interest both children opened (D8 — symmetric teardown over the whole tree).

use std::collections::BTreeSet;

use crate::{FeedOpenError, NmpApp};
use nmp_core::substrate::{KernelEvent, ObservedProjectionRegistrar};
use nmp_feed::RootAdmission;
use nmp_planner::InterestShape;

use super::resolve::{resolve_scope, SetOp};
use super::source::{ExtraAcquisition, LiveShape, ReducedSource};

/// Resolve a binary set-algebra scope by recursing into both children.
pub(super) fn resolve_set_op(
    app: &NmpApp,
    op: SetOp,
    left: &nmp_feed::FeedScope,
    right: &nmp_feed::FeedScope,
    kinds: &BTreeSet<u32>,
) -> Result<ReducedSource, FeedOpenError> {
    let l = resolve_scope(app, left, kinds)?;
    // If the right child fails AFTER the left already registered resolver
    // observers, revoke the left's observers so nothing leaks (fail-closed: a
    // failed open registers nothing — D8).
    let r = match resolve_scope(app, right, kinds) {
        Ok(r) => r,
        Err(e) => {
            for id in &l.resolver_observer_ids {
                app.close_observed_projection(*id);
            }
            for id in &l.identity_observer_ids {
                app.unregister_identity_change_observer(*id);
            }
            for teardown in l.resolver_teardown {
                teardown();
            }
            return Err(e);
        }
    };
    let op_session_identity = l.op_session_identity.combine(r.op_session_identity);

    // ── Admission combine over the children's LIVE, EVENT-AWARE predicates ──
    //
    // Event-aware (#1740 step 3): combining whole-event predicates (not just
    // authors) is what lets a MIXED tag+author composite — e.g.
    // `Intersection(Tag, ContactList)` or `Difference(ContactList, Tag)` —
    // evaluate BOTH the `#t` tag and author membership. An author-only combine
    // could only treat the tag side as `Any`, silently mis-admitting.
    let admission: RootAdmission = {
        let lp = l.admission.clone();
        let rp = r.admission.clone();
        match op {
            SetOp::Union => std::sync::Arc::new(move |ev: &KernelEvent| lp(ev) || rp(ev)),
            SetOp::Intersection => std::sync::Arc::new(move |ev: &KernelEvent| lp(ev) && rp(ev)),
            SetOp::Difference => std::sync::Arc::new(move |ev: &KernelEvent| lp(ev) && !rp(ev)),
        }
    };
    let attribution: nmp_feed::FollowPredicate = {
        let lp = l.attribution.clone();
        let rp = r.attribution.clone();
        match op {
            SetOp::Union => std::sync::Arc::new(move |pubkey: &str| lp(pubkey) || rp(pubkey)),
            SetOp::Intersection => {
                std::sync::Arc::new(move |pubkey: &str| lp(pubkey) && rp(pubkey))
            }
            SetOp::Difference => std::sync::Arc::new(move |pubkey: &str| lp(pubkey) && !rp(pubkey)),
        }
    };

    // ── Acquisition combine ───────────────────────────────────────────────
    //
    // Union/Intersection: acquire from BOTH children (you must see both sides'
    // events to evaluate the OR/AND). Difference: acquire only the left side —
    // the right side is a pure exclusion filter, never a row source.
    let mut interests = l.interests.clone();
    if !matches!(op, SetOp::Difference) {
        interests.extend(r.interests.clone());
    }

    // ── Live pull shape — merge the children's author sets ────────────────
    let live_shape: LiveShape = {
        let ls = l.live_shape.clone();
        let rs = r.live_shape.clone();
        let kinds = kinds.clone();
        let include_right = !matches!(op, SetOp::Difference);
        std::sync::Arc::new(move || merge_shapes(&ls, &rs, include_right, &kinds))
    };

    // ── Extra acquisition — left always; right only when its rows are sources ─
    //
    // Difference excludes the right child's acquisition (it is a pure exclusion
    // filter), so the right side's extra acquisition is dropped — matching the
    // `interests` combine above.
    let extra_acquisition: ExtraAcquisition = {
        let le = l.extra_acquisition.clone();
        let re = r.extra_acquisition.clone();
        let include_right = !matches!(op, SetOp::Difference);
        std::sync::Arc::new(move || {
            let mut shapes = le();
            if include_right {
                shapes.extend(re());
            }
            shapes
        })
    };

    // ── Reactive reset + teardown flow up from BOTH children ──────────────
    //
    // Both children's reset hooks + observer ids flow up even for Difference: the
    // right side must stay reactive so its exclusion (AndNot) tracks changes.
    let mut reset_hooks = l.reset_hooks;
    reset_hooks.extend(r.reset_hooks);
    let mut source_effect_hooks = l.source_effect_hooks;
    source_effect_hooks.extend(r.source_effect_hooks);
    let mut resolver_observer_ids = l.resolver_observer_ids;
    resolver_observer_ids.extend(r.resolver_observer_ids);
    let mut identity_observer_ids = l.identity_observer_ids;
    identity_observer_ids.extend(r.identity_observer_ids);
    let mut resolver_teardown = l.resolver_teardown;
    resolver_teardown.extend(r.resolver_teardown);
    let active_follow_set = l.active_follow_set.or(r.active_follow_set);

    Ok(ReducedSource {
        op_session_identity,
        admission,
        attribution,
        interests,
        live_shape,
        extra_acquisition,
        reset_hooks,
        source_effect_hooks,
        resolver_observer_ids,
        identity_observer_ids,
        resolver_teardown,
        active_follow_set,
    })
}

/// Merge two live acquisition shapes by unioning their author + kind sets. A
/// `None` from a child contributes nothing; if both are `None`, the result is
/// `None` (no pull). For `Difference` the right child is excluded entirely.
fn merge_shapes(
    left: &LiveShape,
    right: &LiveShape,
    include_right: bool,
    _kinds: &BTreeSet<u32>,
) -> Option<InterestShape> {
    let l = left();
    let r = if include_right { right() } else { None };
    match (l, r) {
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nmp_planner::NaddrCoord;

    use super::*;

    #[test]
    fn union_live_shape_preserves_pointer_target_event_ids_and_addresses() {
        let left: LiveShape = Arc::new(|| {
            Some(InterestShape {
                authors: BTreeSet::from(["followed-author".to_string()]),
                kinds: BTreeSet::from([30_023]),
                ..InterestShape::default()
            })
        });
        let right: LiveShape = Arc::new(|| {
            Some(InterestShape {
                event_ids: BTreeSet::from(["article-event-id".to_string()]),
                addresses: BTreeSet::from([NaddrCoord {
                    pubkey: "article-author".to_string(),
                    kind: 30_023,
                    d_tag: "article".to_string(),
                }]),
                kinds: BTreeSet::from([30_023]),
                ..InterestShape::default()
            })
        });

        let merged =
            merge_shapes(&left, &right, true, &BTreeSet::from([30_023])).expect("merged shape");
        assert_eq!(
            merged.authors,
            BTreeSet::from(["followed-author".to_string()])
        );
        assert_eq!(
            merged.event_ids,
            BTreeSet::from(["article-event-id".to_string()])
        );
        assert_eq!(
            merged.addresses,
            BTreeSet::from([NaddrCoord {
                pubkey: "article-author".to_string(),
                kind: 30_023,
                d_tag: "article".to_string(),
            }])
        );
    }
}
