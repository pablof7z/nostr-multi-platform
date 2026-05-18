//! Wire-emitter — `CompiledPlan` → `Vec<WireFrame>` diff.
//!
//! Given a prior plan and a next plan, computes the minimum set of `REQ` and
//! `CLOSE` frames that transitions the wire from the prior to the next. Per
//! recompilation.md §4.3 idempotence contract: `plan_diff(P, P)` returns an
//! empty vector.
//!
//! ## Sub-id stability
//!
//! `sub_id_for` derives a stable wire sub-id from `(plan_id, shape's
//! canonical_filter_hash)`. The same shape appearing in two consecutive plans
//! gets the same sub-id and is therefore a no-op in the diff; a shape that
//! drops out produces a CLOSE; a shape that appears produces a REQ.
//!
//! ## D8 cost shape
//!
//! `plan_diff` is O(N_prior + N_next) where N is the number of `SubShape`s.
//! No per-event allocation; only one `BTreeSet` and one `Vec::with_capacity`.

use std::collections::BTreeSet;

use crate::planner::{
    CompiledPlan, InterestId, InterestLifecycle, InterestShape, LogicalInterest, RelayUrl,
    SubShape,
};

/// A frame to push onto the wire.
#[derive(Clone, Debug)]
pub enum WireFrame {
    /// `["REQ", sub_id, filter]` for the given relay.
    Req {
        relay_url: RelayUrl,
        sub_id: String,
        filter_json: String,
        interest_id: InterestId,
        lifecycle: InterestLifecycle,
    },
    /// `["CLOSE", sub_id]` for the given relay.
    Close {
        relay_url: RelayUrl,
        sub_id: String,
    },
}

/// Compute the wire-frame delta between `prior` and `next` plans.
///
/// Both arguments are `Option<&CompiledPlan>` so the same function handles
/// the initial-compile case (prior = None → all REQs) and the teardown case
/// (next = None → all CLOSEs). `next_interests` is consulted to determine
/// lifecycle metadata for the REQ frames.
pub fn plan_diff(
    prior: Option<&CompiledPlan>,
    next: Option<&CompiledPlan>,
    next_interests: &[LogicalInterest],
) -> Vec<WireFrame> {
    let prior_sub_ids = collect_sub_ids(prior);
    let next_sub_ids = collect_sub_ids(next);

    let mut frames = Vec::new();

    // CLOSE for sub_ids in prior but not in next.
    if let Some(plan) = prior {
        for (relay_url, relay_plan) in &plan.per_relay {
            for shape in &relay_plan.sub_shapes {
                let sub_id = sub_id_for(&plan.plan_id, shape);
                if !next_sub_ids.contains(&sub_id) {
                    frames.push(WireFrame::Close {
                        relay_url: relay_url.clone(),
                        sub_id,
                    });
                }
            }
        }
    }

    // REQ for sub_ids in next but not in prior.
    if let Some(plan) = next {
        for (relay_url, relay_plan) in &plan.per_relay {
            for shape in &relay_plan.sub_shapes {
                let sub_id = sub_id_for(&plan.plan_id, shape);
                if !prior_sub_ids.contains(&sub_id) {
                    frames.push(emit_req(relay_url.clone(), shape, next_interests, sub_id));
                }
            }
        }
    }

    frames
}

fn collect_sub_ids(plan: Option<&CompiledPlan>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if let Some(plan) = plan {
        for relay_plan in plan.per_relay.values() {
            for shape in &relay_plan.sub_shapes {
                out.insert(sub_id_for(&plan.plan_id, shape));
            }
        }
    }
    out
}

fn emit_req(
    relay_url: RelayUrl,
    shape: &SubShape,
    interests: &[LogicalInterest],
    sub_id: String,
) -> WireFrame {
    let interest_id = shape
        .originating_interests
        .first()
        .cloned()
        .unwrap_or(InterestId(0));
    let lifecycle = lifecycle_for_shape(shape, interests);
    let filter_json = filter_json_for(&shape.shape);
    WireFrame::Req {
        relay_url,
        sub_id,
        filter_json,
        interest_id,
        lifecycle,
    }
}

/// Derive a stable wire sub-id for `(plan_id, shape)`. Two identical shapes
/// in consecutive plans get the same sub-id — the diff treats them as a
/// no-op. A shape merging differently across recompiles gets a new sub-id
/// because the underlying `canonical_filter_hash` would change.
///
/// We deliberately do NOT include the plan_id in the sub-id — that would
/// force every shape to be CLOSE+REQ'd on every plan-id change, defeating
/// the diff. Instead, the sub-id is derived purely from the shape's hash.
pub fn sub_id_for(_plan_id: &str, shape: &SubShape) -> String {
    format!("sub-{}", shape.canonical_filter_hash)
}

/// Determine the lifecycle to apply to a merged sub-shape.
///
/// Rule 6 of the lattice (`lattice::rules::rule6_lifecycle_equality`) refuses
/// to merge shapes with different lifecycles, so all originating interests
/// share one lifecycle. We pick the first originating interest's lifecycle;
/// fallback to `Tailing` if the originating set is empty (defensive).
pub fn lifecycle_for_shape(
    shape: &SubShape,
    interests: &[LogicalInterest],
) -> InterestLifecycle {
    for origin in &shape.originating_interests {
        if let Some(i) = interests.iter().find(|i| &i.id == origin) {
            return i.lifecycle.clone();
        }
    }
    InterestLifecycle::Tailing
}

/// Serialise an `InterestShape` into the Nostr filter JSON object form.
///
/// This is a minimal serializer — only the fields a relay accepts. Internal
/// fields like `relay_pin` and `addresses` are translated to wire-equivalent
/// forms (`#a` tags for addresses) or omitted (`relay_pin` is client-side
/// only — it never appears on the wire).
pub fn filter_json_for(shape: &InterestShape) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !shape.authors.is_empty() {
        let arr: Vec<String> = shape.authors.iter().map(|a| format!("\"{a}\"")).collect();
        parts.push(format!("\"authors\":[{}]", arr.join(",")));
    }
    if !shape.kinds.is_empty() {
        let arr: Vec<String> = shape.kinds.iter().map(|k| k.to_string()).collect();
        parts.push(format!("\"kinds\":[{}]", arr.join(",")));
    }
    if !shape.event_ids.is_empty() {
        let arr: Vec<String> = shape.event_ids.iter().map(|e| format!("\"{e}\"")).collect();
        parts.push(format!("\"ids\":[{}]", arr.join(",")));
    }
    for (tag_key, values) in &shape.tags {
        let arr: Vec<String> = values.iter().map(|v| format!("\"{v}\"")).collect();
        parts.push(format!("\"#{tag_key}\":[{}]", arr.join(",")));
    }
    if !shape.addresses.is_empty() {
        let arr: Vec<String> = shape
            .addresses
            .iter()
            .map(|a| format!("\"{}:{}:{}\"", a.kind, a.pubkey, a.d_tag))
            .collect();
        parts.push(format!("\"#a\":[{}]", arr.join(",")));
    }
    if let Some(since) = shape.since {
        parts.push(format!("\"since\":{since}"));
    }
    if let Some(until) = shape.until {
        parts.push(format!("\"until\":{until}"));
    }
    if let Some(limit) = shape.limit {
        parts.push(format!("\"limit\":{limit}"));
    }
    format!("{{{}}}", parts.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::{
        InMemoryMailboxCache, InterestId, InterestScope, MailboxSnapshot, SubscriptionCompiler,
    };

    fn pubkey(s: &str) -> String {
        format!("{s:0>64}").chars().take(64).collect()
    }

    fn ti(id: u64, authors: &[&str], lc: InterestLifecycle) -> LogicalInterest {
        LogicalInterest {
            id: InterestId(id),
            scope: InterestScope::Global,
            shape: InterestShape {
                authors: authors.iter().map(|a| pubkey(a)).collect(),
                kinds: [1u32].into_iter().collect(),
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle: lc,
        }
    }

    #[test]
    fn diff_against_empty_emits_all_reqs() {
        let mut cache = InMemoryMailboxCache::new();
        cache.put(
            pubkey("a"),
            MailboxSnapshot {
                write_relays: vec!["wss://r1".to_string()],
                read_relays: vec![],
                both_relays: vec![],
            },
        );
        let indexer = vec!["wss://ix".to_string()];
        let compiler = SubscriptionCompiler::new(&cache, &indexer);
        let interests = vec![ti(1, &["a"], InterestLifecycle::Tailing)];
        let plan = compiler.compile(&interests).expect("compile");

        let frames = plan_diff(None, Some(&plan), &interests);
        let reqs = frames
            .iter()
            .filter(|f| matches!(f, WireFrame::Req { .. }))
            .count();
        let closes = frames
            .iter()
            .filter(|f| matches!(f, WireFrame::Close { .. }))
            .count();
        assert!(reqs >= 1);
        assert_eq!(closes, 0);
    }

    #[test]
    fn diff_identical_is_empty() {
        let mut cache = InMemoryMailboxCache::new();
        cache.put(
            pubkey("a"),
            MailboxSnapshot {
                write_relays: vec!["wss://r1".to_string()],
                read_relays: vec![],
                both_relays: vec![],
            },
        );
        let indexer = vec!["wss://ix".to_string()];
        let compiler = SubscriptionCompiler::new(&cache, &indexer);
        let interests = vec![ti(1, &["a"], InterestLifecycle::Tailing)];
        let plan = compiler.compile(&interests).expect("compile");
        let frames = plan_diff(Some(&plan), Some(&plan), &interests);
        assert!(frames.is_empty(), "identical plans → empty diff");
    }
}
