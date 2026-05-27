//! Wire-emitter — `CompiledPlan` → `Vec<WireFrame>` diff.
//!
//! Given a prior plan and a next plan, computes the minimum set of `REQ` and
//! `CLOSE` frames that transitions the wire from the prior to the next. Per
//! recompilation.md §4.3 idempotence contract: `plan_diff(P, P)` returns an
//! empty vector.
//!
//! ## Sub-id stability
//!
//! `sub_id_for` derives a stable wire sub-id from the shape's
//! `canonical_filter_hash`. Per NIP-01 §1, subscription ids are
//! per-connection: the same filter on two different relay connections may
//! legitimately reuse the same sub-id string. Therefore `sub_id_for` does NOT
//! include the relay URL — the wire id is stable for the filter, not the relay.
//!
//! ## Relay-scoped diff keying
//!
//! The diff sets are keyed by `(relay_url, sub_id)` — NOT by `sub_id` alone.
//! This is the critical distinction from the wire id: a sub is "present" only
//! on the specific relay that carries it. Without relay-scoped keying, two
//! relays sharing the same filter hash would share a single diff entry, causing:
//! - **Dead-relay exclusion**: surviving relay contributes `sub_id` → no CLOSE
//!   emitted for the dead relay.
//! - **App-relay add**: new relay's `sub_id` already present via prior relay →
//!   REQ skipped on the newly-added relay.
//!
//! Codex F-CROSS-1 (HIGH) — fixed here.
//!
//! ## D8 cost shape
//!
//! `plan_diff` is `O(N_prior` + `N_next`) where N is the number of `SubShape`s.
//! No per-event allocation; two `BTreeSet`s and one `Vec::with_capacity`.

use std::collections::BTreeSet;

use crate::planner::{
    CompiledPlan, InterestId, InterestLifecycle, InterestShape, LogicalInterest, RelayUrl, SubShape,
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
    Close { relay_url: RelayUrl, sub_id: String },
}

/// Compute the wire-frame delta between `prior` and `next` plans.
///
/// Both arguments are `Option<&CompiledPlan>` so the same function handles
/// the initial-compile case (prior = None → all REQs) and the teardown case
/// (next = None → all CLOSEs). `next_interests` is consulted to determine
/// lifecycle metadata for the REQ frames.
#[must_use]
pub fn plan_diff(
    prior: Option<&CompiledPlan>,
    next: Option<&CompiledPlan>,
    next_interests: &[LogicalInterest],
) -> Vec<WireFrame> {
    // Keys are (relay_url, sub_id) — relay-scoped so that two relays carrying
    // the same filter hash are tracked independently. See module doc for the
    // distinction between the wire sub-id string (per-filter stable) and the
    // diff key (per-relay, per-filter — the unit of subscription presence).
    let prior_keys = collect_relay_sub_keys(prior);
    let next_keys = collect_relay_sub_keys(next);

    let mut frames = Vec::new();

    // CLOSE for (relay, sub_id) pairs in prior but not in next.
    if let Some(plan) = prior {
        for (relay_url, relay_plan) in &plan.per_relay {
            for shape in &relay_plan.sub_shapes {
                let sub_id = sub_id_for(&plan.plan_id, shape);
                if !next_keys.contains(&(relay_url.clone(), sub_id.clone())) {
                    frames.push(WireFrame::Close {
                        relay_url: relay_url.clone(),
                        sub_id,
                    });
                }
            }
        }
    }

    // REQ for (relay, sub_id) pairs in next but not in prior.
    if let Some(plan) = next {
        for (relay_url, relay_plan) in &plan.per_relay {
            for shape in &relay_plan.sub_shapes {
                let sub_id = sub_id_for(&plan.plan_id, shape);
                if !prior_keys.contains(&(relay_url.clone(), sub_id.clone())) {
                    frames.push(emit_req(relay_url.clone(), shape, next_interests, sub_id));
                }
            }
        }
    }

    frames
}

/// Collect all `(relay_url, sub_id)` pairs from a plan.
///
/// Keying by the pair — not by `sub_id` alone — is what makes the diff
/// relay-scoped: a sub-shape present on relay A is distinct from the same
/// sub-shape on relay B, even when they share a `canonical_filter_hash`.
fn collect_relay_sub_keys(plan: Option<&CompiledPlan>) -> BTreeSet<(RelayUrl, String)> {
    let mut out = BTreeSet::new();
    if let Some(plan) = plan {
        for (relay_url, relay_plan) in &plan.per_relay {
            for shape in &relay_plan.sub_shapes {
                out.insert((relay_url.clone(), sub_id_for(&plan.plan_id, shape)));
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
/// We deliberately do NOT include the `plan_id` in the sub-id — that would
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
pub fn lifecycle_for_shape(shape: &SubShape, interests: &[LogicalInterest]) -> InterestLifecycle {
    for origin in &shape.originating_interests {
        if let Some(i) = interests.iter().find(|i| &i.id == origin) {
            return i.lifecycle.clone();
        }
    }
    InterestLifecycle::Tailing
}

/// Serialise an `InterestShape` into the Nostr filter JSON object form.
///
/// Delegates to `nostr::Filter`'s builder + serde serializer so escaping,
/// field ordering, and NIP-01 conformance match the canonical `nostr` crate
/// implementation. The previous hand-rolled string concatenation had a real
/// correctness risk — tag values were interpolated into JSON with no escaping
/// (a `"` or `\` in a tag value would have produced malformed JSON).
///
/// ## Field mapping
///
/// - `authors` → `Filter::authors` (parses each `Pubkey` hex via
///   `PublicKey::from_hex`; entries that fail strict hex parsing are dropped
///   — see "behaviour notes" below).
/// - `kinds` → `Filter::kinds` (`u32 → u16` cast: NIP-01 kinds fit in `u16`).
/// - `event_ids` → `Filter::ids` (parses each `EventId` hex; failures dropped).
/// - `tags` → `Filter::custom_tags`, one entry per single-letter `TagKey`.
///   Multi-character tag keys are unrepresentable in NIP-01 and are dropped.
/// - `addresses` → `#a` via `Filter::coordinates` (matches the prior
///   hand-rolled `"kind:pubkey:d-tag"` serialisation through `Coordinate`'s
///   `Display`).
/// - `since` / `until` → `Filter::since` / `Filter::until` via `Timestamp::from_secs`.
/// - `limit` → `Filter::limit` (`u32 → usize` cast, always lossless on the
///   targets we ship).
///
/// Client-side-only fields (`relay_pin`, `p_tag_routing`) never appear on the
/// wire and are deliberately omitted, exactly as before.
///
/// ## Behaviour notes vs. the prior hand-rolled version
///
/// - Hex-validation gap is closed: invalid hex authors / event ids are now
///   dropped at serialise time rather than being smuggled onto the wire as
///   malformed strings. Tests that wanted observable behaviour use valid hex
///   (`"ab".repeat(32)`, etc.); fixtures that pad arbitrary identifiers to
///   64 chars (`pubkey("overlap_a")`) intentionally exercise the
///   relay-routing path, not the filter-string path.
/// - JSON field ordering changes (nostr emits `ids, authors, kinds, since,
///   until, limit, #<tags>` in struct-declaration order; the hand-rolled
///   version emitted `authors` first). No caller asserts on byte-exact JSON
///   shape; `kernel::replay` re-parses via `serde_json::from_str::<Value>`
///   so ordering is irrelevant downstream.
/// - `canonical_filter_hash` is independent — it hashes the `InterestShape`
///   struct directly via `serde_json::to_string`, not this function's output.
///   `plan_id` stability is preserved.
pub fn filter_json_for(shape: &InterestShape) -> String {
    use nostr::nips::nip01::Coordinate;
    use nostr::{EventId as NostrEventId, Filter, Kind, PublicKey, SingleLetterTag, Timestamp};

    let mut filter = Filter::new();

    if !shape.authors.is_empty() {
        let authors: Vec<PublicKey> = shape
            .authors
            .iter()
            .filter_map(|a| PublicKey::from_hex(a).ok())
            .collect();
        if !authors.is_empty() {
            filter = filter.authors(authors);
        }
    }

    if !shape.kinds.is_empty() {
        // NIP-01 kinds fit in u16; the cast is lossless for every kind
        // defined by the spec or in use across the codebase.
        filter = filter.kinds(shape.kinds.iter().map(|k| Kind::from(*k as u16)));
    }

    if !shape.event_ids.is_empty() {
        let ids: Vec<NostrEventId> = shape
            .event_ids
            .iter()
            .filter_map(|e| NostrEventId::from_hex(e).ok())
            .collect();
        if !ids.is_empty() {
            filter = filter.ids(ids);
        }
    }

    for (tag_key, values) in &shape.tags {
        // NIP-01 generic tag keys are single ASCII letters. Multi-character
        // keys are unrepresentable and silently dropped — no callsite in the
        // workspace constructs one (all uses: `p`, `e`, `a`, `t`, `h`, `d`).
        let mut chars = tag_key.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            continue;
        };
        let Ok(letter) = SingleLetterTag::from_char(c) else {
            continue;
        };
        filter = filter.custom_tags(letter, values.iter().cloned());
    }

    if !shape.addresses.is_empty() {
        let coords: Vec<Coordinate> = shape
            .addresses
            .iter()
            .filter_map(|a| {
                let pk = PublicKey::from_hex(&a.pubkey).ok()?;
                Some(Coordinate {
                    kind: Kind::from(a.kind as u16),
                    public_key: pk,
                    identifier: a.d_tag.clone(),
                })
            })
            .collect();
        if !coords.is_empty() {
            filter = filter.coordinates(coords.iter());
        }
    }

    if let Some(since) = shape.since {
        filter = filter.since(Timestamp::from_secs(since));
    }
    if let Some(until) = shape.until {
        filter = filter.until(Timestamp::from_secs(until));
    }
    if let Some(limit) = shape.limit {
        filter = filter.limit(limit as usize);
    }

    // `serde_json::to_string` on `Filter` cannot realistically fail (no
    // non-string map keys, no NaN/Infinity floats), but fall back to an empty
    // filter JSON rather than panic across the FFI boundary (D6).
    serde_json::to_string(&filter).unwrap_or_else(|_| "{}".to_string())
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
            is_indexer_discovery: false,
        }
    }

    // ── F-CROSS-1: relay-scoped diff keying ─────────────────────────────────

    fn snap(write_relays: Vec<&str>) -> MailboxSnapshot {
        MailboxSnapshot {
            write_relays: write_relays.into_iter().map(str::to_string).collect(),
            read_relays: vec![],
            both_relays: vec![],
        }
    }

    fn req_relays(frames: &[WireFrame]) -> std::collections::BTreeSet<String> {
        frames
            .iter()
            .filter_map(|f| match f {
                WireFrame::Req { relay_url, .. } => Some(relay_url.clone()),
                _ => None,
            })
            .collect()
    }

    fn close_relays(frames: &[WireFrame]) -> std::collections::BTreeSet<String> {
        frames
            .iter()
            .filter_map(|f| match f {
                WireFrame::Close { relay_url, .. } => Some(relay_url.clone()),
                _ => None,
            })
            .collect()
    }

    /// One author → two write relays (same filter hash). Both relays must
    /// receive a REQ on the initial diff (prior = None).
    #[test]
    fn plan_diff_overlapping_filter_two_relays_emits_per_relay_frames() {
        let mut cache = InMemoryMailboxCache::new();
        cache.put(
            pubkey("overlap_a"),
            snap(vec!["wss://relay-x.example", "wss://relay-y.example"]),
        );
        let interests = vec![ti(1, &["overlap_a"], InterestLifecycle::Tailing)];
        let plan = SubscriptionCompiler::new(&cache, &[])
            .compile(&interests)
            .expect("compile");
        assert!(plan.per_relay.len() >= 2, "need both relays in plan");
        let frames = plan_diff(None, Some(&plan), &interests);
        let reqs = req_relays(&frames);
        assert!(
            reqs.contains("wss://relay-x.example"),
            "relay-x must get REQ; {reqs:?}"
        );
        assert!(
            reqs.contains("wss://relay-y.example"),
            "relay-y must get REQ; {reqs:?}"
        );
    }

    /// Same filter on two relays. One relay removed in next plan.
    /// CLOSE must be emitted for the removed relay only, not for the survivor.
    /// Fails on current code: surviving relay still contributes the same sub_id
    /// to the global next-set → no CLOSE emitted for the removed relay.
    #[test]
    fn plan_diff_dead_relay_with_shared_filter_emits_close() {
        let mut cache = InMemoryMailboxCache::new();
        cache.put(
            pubkey("dead_b"),
            snap(vec![
                "wss://relay-alive.example",
                "wss://relay-dead.example",
            ]),
        );
        let interests = vec![ti(1, &["dead_b"], InterestLifecycle::Tailing)];
        let prior_plan = SubscriptionCompiler::new(&cache, &[])
            .compile(&interests)
            .expect("prior");
        assert!(prior_plan
            .per_relay
            .contains_key("wss://relay-alive.example"));
        assert!(prior_plan
            .per_relay
            .contains_key("wss://relay-dead.example"));

        let mut cache2 = InMemoryMailboxCache::new();
        cache2.put(pubkey("dead_b"), snap(vec!["wss://relay-alive.example"]));
        let next_plan = SubscriptionCompiler::new(&cache2, &[])
            .compile(&interests)
            .expect("next");

        let closes = close_relays(&plan_diff(Some(&prior_plan), Some(&next_plan), &interests));
        assert!(
            closes.contains("wss://relay-dead.example"),
            "CLOSE for dead relay; {closes:?}"
        );
        assert!(
            !closes.contains("wss://relay-alive.example"),
            "no CLOSE for alive relay; {closes:?}"
        );
    }

    /// Author already on NIP-65 relay X. App relay Y added in next plan.
    /// Y must receive a REQ even though it carries the same filter hash as X.
    /// Fails on current code: sub_id already present in prior global set → REQ skipped.
    #[test]
    fn plan_diff_app_relay_add_for_already_routed_author_emits_req() {
        let mut cache = InMemoryMailboxCache::new();
        cache.put(pubkey("app_a"), snap(vec!["wss://relay-nip65.example"]));
        let interests = vec![ti(1, &["app_a"], InterestLifecycle::Tailing)];
        let prior_plan = SubscriptionCompiler::new(&cache, &[])
            .compile(&interests)
            .expect("prior");
        assert!(prior_plan
            .per_relay
            .contains_key("wss://relay-nip65.example"));

        let app_relays = vec!["wss://app-relay-y.example".to_string()];
        let next_plan = SubscriptionCompiler::with_relays(&cache, &[], &[], &app_relays)
            .compile(&interests)
            .expect("next");
        assert!(
            next_plan
                .per_relay
                .contains_key("wss://app-relay-y.example"),
            "next plan must include app relay; got {:?}",
            next_plan.per_relay.keys().collect::<Vec<_>>()
        );

        let reqs = req_relays(&plan_diff(Some(&prior_plan), Some(&next_plan), &interests));
        assert!(
            reqs.contains("wss://app-relay-y.example"),
            "app relay Y must get REQ; {reqs:?}"
        );
        assert!(
            !reqs.contains("wss://relay-nip65.example"),
            "NIP-65 relay X must not get redundant REQ; {reqs:?}"
        );
    }

    /// Regression: unique (author, relay) pairs still behave correctly with relay-scoped keying.
    /// Two authors, each with a unique write relay. Drop one → CLOSE on that relay only.
    #[test]
    fn plan_diff_unique_pairs_regression_still_works() {
        let mut cache = InMemoryMailboxCache::new();
        cache.put(pubkey("unique_a"), snap(vec!["wss://unique-r1.example"]));
        cache.put(pubkey("unique_b"), snap(vec!["wss://unique-r2.example"]));
        let interests = vec![
            ti(1, &["unique_a"], InterestLifecycle::Tailing),
            ti(2, &["unique_b"], InterestLifecycle::Tailing),
        ];
        let prior_plan = SubscriptionCompiler::new(&cache, &[])
            .compile(&interests)
            .expect("prior");
        let first_reqs = req_relays(&plan_diff(None, Some(&prior_plan), &interests));
        assert!(first_reqs.contains("wss://unique-r1.example"), "r1 REQ");
        assert!(first_reqs.contains("wss://unique-r2.example"), "r2 REQ");

        let mut cache2 = InMemoryMailboxCache::new();
        cache2.put(pubkey("unique_a"), snap(vec!["wss://unique-r1.example"]));
        let interests2 = vec![ti(1, &["unique_a"], InterestLifecycle::Tailing)];
        let next_plan = SubscriptionCompiler::new(&cache2, &[])
            .compile(&interests2)
            .expect("next");
        let closes = close_relays(&plan_diff(Some(&prior_plan), Some(&next_plan), &interests2));
        assert!(
            closes.contains("wss://unique-r2.example"),
            "r2 CLOSE; {closes:?}"
        );
        assert!(
            !closes.contains("wss://unique-r1.example"),
            "no r1 CLOSE; {closes:?}"
        );
    }

    // ── existing tests ───────────────────────────────────────────────────────

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
