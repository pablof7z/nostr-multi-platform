//! Applesauce-style relay-selection optimizer — greedy weighted max-coverage
//! with a per-author redundancy cap.
//!
//! ## Problem
//!
//! The naive plan produced by [`SubscriptionCompiler`] connects to every NIP-65
//! write-relay declared by every follow. In a real test this was 287 relays for
//! 1048 follows — a connection storm that wastes battery, file descriptors, and
//! relay backpressure budget. The pareto principle applies: a small minority of
//! popular relays already covers the overwhelming majority of follows.
//!
//! This pass reduces the relay set to roughly `max_connections` (e.g. ~30) by
//! solving a weighted max-coverage problem with a redundancy cap: each author
//! is "covered" by at most `max_per_user` selected relays. The cap prevents
//! the algorithm from spending all its connection budget on the head of the
//! distribution (everyone declares the top 3 relays) while ignoring the long
//! tail of authors who only declare niche relays.
//!
//! ## Algorithm — applesauce-pure
//!
//! Mirrors `selectOptimalRelays` from the applesauce TypeScript library
//! (`@hzrd149/applesauce-core/src/helpers/relay-selection.ts`). We deliberately
//! omit the NDK-style "boost already-connected relays" tiebreak — in production
//! that tiebreak creates feedback churn where the selector's output feeds back
//! into the input (a relay selected once stays sticky even when its coverage
//! score drops below the cap, because reconnection counts as a vote).
//!
//! ```text
//! invert per_relay → pool: author → set<relay>
//! while pool non-empty AND |selected| < max_connections:
//!     score each remaining relay by uncovered-author count
//!     pick relay with highest count; deterministic tiebreak on URL
//!     for each author covered by the winner:
//!         hits[author] += 1
//!         record (winner, author) in selections
//!         if hits[author] >= max_per_user: retire author from pool
//!         else: remove winner from that author's pool entry
//! project survivors back onto the plan
//! ```
//!
//! ## Plan-shape integration (Option A)
//!
//! The public entry point [`apply_selection`] operates directly on
//! [`CompiledPlan`]:
//!
//! 1. Collect `(relay → union of all sub_shape author sets)` into a working map.
//! 2. Run the greedy algorithm.
//! 3. Drop relay entries not in `selected`.
//! 4. For each surviving relay, intersect each sub-shape's `authors` set with
//!    the authors that were covered by *this* relay during the loop (the
//!    selection oracle). Empty sub-shapes are dropped; relays whose sub-shape
//!    list is now empty are also dropped.
//! 5. Call [`SubShape::recompute_hash`] on any sub-shape whose author set
//!    actually changed. This is the M4 precedent — post-compile mutators MUST
//!    recompute the canonical filter hash so the wire-emitter's diff emits the
//!    new REQ frame (`docs/perf/codex-reviews/076173d.md` P1 bug).
//!
//! ### Wildcard-author sub-shapes
//!
//! Sub-shapes with an empty `authors` set (e.g. gift-wrap inbox `#p` filters,
//! global kind:0 hydration, hashtag firehose) contribute nothing to coverage and
//! have no authors to filter. They are **preserved unchanged** on selected
//! relays: they ride along with the relay's other reasons for existing. If a
//! relay's only sub-shape is a wildcard, it is also preserved while connection
//! budget remains; protocol inboxes must not be optimized away merely because
//! they are tag-scoped rather than author-scoped.
//!
//! ## Plan-id discipline
//!
//! `plan_id` is content-addressed BEFORE post-compile mutation, so this pass
//! does NOT recompute `plan_id`. See `planner/mod.rs` §"Plan-id determinism vs.
//! post-compile mutators" for the full doctrine. `canonical_filter_hash` on
//! each `SubShape` IS recomputed when authors change — that hash is the
//! wire-emitter's diff key.
//!
//! [`SubscriptionCompiler`]: super::compiler::SubscriptionCompiler

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::interest::{Pubkey, RelayUrl};
use super::plan::CompiledPlan;

// ─── Public API ──────────────────────────────────────────────────────────────

/// Apply greedy weighted max-coverage relay selection to a compiled plan.
///
/// Mutates `plan` in place:
/// - Drops relays whose authors are entirely covered by other surviving relays.
/// - Filters each surviving sub-shape's `authors` set to keep only authors that
///   the algorithm decided this relay should serve.
/// - Recomputes `canonical_filter_hash` on every sub-shape whose author set
///   actually changed.
/// - Drops sub-shapes whose author set became empty (note: wildcard sub-shapes
///   that started empty are *not* affected — they had no authors to filter).
/// - Drops relay entries whose sub-shape list became empty.
///
/// # Arguments
///
/// * `plan` — the freshly-compiled plan from [`SubscriptionCompiler`]; this
///   function does NOT change `plan.plan_id` (per the plan-id discipline; see
///   module docs).
/// * `max_connections` — upper bound on the number of relays in the reduced
///   plan. Real usage: ~30.
/// * `max_per_user` — per-author redundancy cap. Each follow is served by at
///   most this many relays. Real usage: 2.
///
/// # No-op cases
///
/// - `plan.per_relay` is already smaller than `max_connections` AND every
///   author appears on ≤ `max_per_user` relays → nothing to do, but the
///   algorithm still runs (its output equals its input in that case).
/// - `max_connections == 0` → drops all relays. (Probably a config bug; the
///   caller is responsible for clamping.)
/// - `max_per_user == 0` → drops all relays (no relay may cover any author).
///   Same caller caveat.
///
/// [`SubscriptionCompiler`]: super::compiler::SubscriptionCompiler
pub fn apply_selection(plan: &mut CompiledPlan, max_connections: usize, max_per_user: usize) {
    // Stage 1: extract the (relay → union-of-author-sets) shape the algorithm
    // wants. Wildcard sub-shapes (empty authors) contribute nothing here; if a
    // relay's only sub-shapes are wildcards, it will not be picked by coverage
    // and will be dropped.
    let per_relay_authors: BTreeMap<RelayUrl, BTreeSet<Pubkey>> = plan
        .per_relay
        .iter()
        .map(|(relay, relay_plan)| {
            let mut union: BTreeSet<Pubkey> = BTreeSet::new();
            for sub in &relay_plan.sub_shapes {
                union.extend(sub.shape.authors.iter().cloned());
            }
            (relay.clone(), union)
        })
        .collect();

    // Stage 2: greedy max-coverage. Returns the (relay → authors-this-relay-serves)
    // oracle.
    let mut selections = greedy_select(&per_relay_authors, max_connections, max_per_user);
    if max_connections > selections.len() && max_per_user > 0 {
        for (relay, authors) in &per_relay_authors {
            if authors.is_empty() && !selections.contains_key(relay) {
                selections.insert(relay.clone(), BTreeSet::new());
                if selections.len() >= max_connections {
                    break;
                }
            }
        }
    }

    // Stage 3: project back. Drop unselected relays; filter author sets on
    // selected ones; recompute hashes where author sets changed.
    let mut new_per_relay = BTreeMap::new();
    for (relay, mut relay_plan) in std::mem::take(&mut plan.per_relay) {
        let Some(allowed_authors) = selections.get(&relay) else {
            // Relay was not chosen — drop it entirely.
            continue;
        };

        // Filter each sub-shape.
        let mut kept_subs = Vec::with_capacity(relay_plan.sub_shapes.len());
        for mut sub in relay_plan.sub_shapes.drain(..) {
            if sub.shape.authors.is_empty() {
                // Wildcard sub-shape — preserve unchanged (it has nothing to filter).
                kept_subs.push(sub);
                continue;
            }
            let before = sub.shape.authors.len();
            sub.shape.authors.retain(|a| allowed_authors.contains(a));
            if sub.shape.authors.is_empty() {
                // All authors filtered out — drop this sub-shape.
                continue;
            }
            if sub.shape.authors.len() != before {
                // Author set changed — wire-emitter MUST see a new sub-id.
                sub.recompute_hash();
            }
            kept_subs.push(sub);
        }

        if kept_subs.is_empty() {
            // Every sub-shape on this relay was filtered to empty — drop.
            continue;
        }
        relay_plan.sub_shapes = kept_subs;
        new_per_relay.insert(relay, relay_plan);
    }
    plan.per_relay = new_per_relay;
}

// ─── Core algorithm ──────────────────────────────────────────────────────────

/// The pure greedy max-coverage routine.
///
/// Returns `selected_relay → set_of_authors_this_relay_will_serve`. Only
/// surviving (relay, author) pairs appear; relays not in the result are
/// excluded from the reduced plan.
///
/// Separated from [`apply_selection`] for unit-testability and as a future
/// re-use point (e.g. an `nmp-testing` audit gate that wants the raw oracle
/// without a `CompiledPlan` in hand).
fn greedy_select(
    per_relay: &BTreeMap<RelayUrl, BTreeSet<Pubkey>>,
    max_connections: usize,
    max_per_user: usize,
) -> BTreeMap<RelayUrl, BTreeSet<Pubkey>> {
    let mut selections: BTreeMap<RelayUrl, BTreeSet<Pubkey>> = BTreeMap::new();

    if max_connections == 0 || max_per_user == 0 {
        return selections;
    }

    // Invert: author → set<relay>. Discard wildcard sub-shapes (empty author
    // sets contribute nothing).
    let mut pool: HashMap<Pubkey, HashSet<RelayUrl>> = HashMap::new();
    for (relay, authors) in per_relay {
        for author in authors {
            pool.entry(author.clone()).or_default().insert(relay.clone());
        }
    }

    let mut hits: HashMap<Pubkey, usize> = HashMap::new();
    let mut selected: HashSet<RelayUrl> = HashSet::new();

    while !pool.is_empty() && selected.len() < max_connections {
        // Compute uncovered coverage per remaining relay.
        let mut coverage: HashMap<&RelayUrl, usize> = HashMap::new();
        for relays in pool.values() {
            for r in relays {
                if selected.contains(r) {
                    continue;
                }
                *coverage.entry(r).or_insert(0) += 1;
            }
        }
        if coverage.is_empty() {
            break;
        }

        // Pick winner: highest coverage; deterministic lexicographic tiebreak.
        // (count ascending → higher count wins; relay-string DESC so that
        // "wss://a..." beats "wss://z..." on tie. The exact direction is
        // irrelevant for correctness; what matters is that the comparator is
        // total and stable across runs.)
        let winner_url: RelayUrl = coverage
            .into_iter()
            .max_by(|a, b| match a.1.cmp(&b.1) {
                std::cmp::Ordering::Equal => b.0.cmp(a.0), // reverse on URL
                ord => ord,
            })
            .map(|(r, _)| r.clone())
            .expect("coverage non-empty checked above"); // doctrine-allow: D6 — coverage emptiness guarded at line 226; max_by on non-empty iter always returns Some

        selected.insert(winner_url.clone());

        // Walk authors snapshot — we mutate `pool` inside the loop.
        let authors_now: Vec<Pubkey> = pool.keys().cloned().collect();
        for author in authors_now {
            let covered = pool
                .get(&author)
                .is_some_and(|relays| relays.contains(&winner_url));
            if !covered {
                continue;
            }

            // Record the (winner, author) decision in the selection oracle —
            // this is the projection target regardless of whether the author
            // then retires or just loses this relay from their pool.
            selections
                .entry(winner_url.clone())
                .or_default()
                .insert(author.clone());

            // Pre-increment-then-compare — see module docs for the applesauce
            // post-increment bug we are deliberately not reproducing.
            let count = hits.entry(author.clone()).or_insert(0);
            *count += 1;
            if *count >= max_per_user {
                pool.remove(&author);
            } else if let Some(relays) = pool.get_mut(&author) {
                relays.remove(&winner_url);
                if relays.is_empty() {
                    pool.remove(&author);
                }
            }
        }
    }

    selections
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::interest::InterestShape;
    use crate::planner::plan::{canonical_filter_hash, RelayPlan, RoutingSource, SubShape};

    /// Build a tiny plan with one sub-shape per relay, where each sub-shape's
    /// `authors` is the given set. Helper for terse tests.
    fn plan_with(relays: &[(&str, &[&str])]) -> CompiledPlan {
        let mut per_relay = BTreeMap::new();
        for (relay, authors) in relays {
            let mut shape = InterestShape::default();
            for a in *authors {
                shape.authors.insert((*a).to_string());
            }
            let hash = canonical_filter_hash(&shape);
            let sub = SubShape {
                shape,
                originating_interests: vec![],
                canonical_filter_hash: hash,
            };
            let mut role_tags = BTreeSet::new();
            role_tags.insert(RoutingSource::Nip65);
            per_relay.insert(
                (*relay).to_string(),
                RelayPlan {
                    relay_url: (*relay).to_string(),
                    role_tags,
                    sub_shapes: vec![sub],
                },
            );
        }
        CompiledPlan {
            plan_id: "test".to_string(),
            per_relay,
            unroutable_authors: BTreeSet::new(),
        }
    }

    #[test]
    fn empty_plan_stays_empty() {
        let mut plan = CompiledPlan::empty("empty");
        apply_selection(&mut plan, 30, 2);
        assert!(plan.per_relay.is_empty());
        assert_eq!(plan.plan_id, "empty", "plan_id must not change");
    }

    #[test]
    fn single_author_single_relay_unchanged() {
        let mut plan = plan_with(&[("wss://a", &["alice"])]);
        let before_hash = plan.per_relay["wss://a"].sub_shapes[0].canonical_filter_hash.clone();
        apply_selection(&mut plan, 30, 2);
        assert_eq!(plan.per_relay.len(), 1);
        assert_eq!(plan.per_relay["wss://a"].sub_shapes.len(), 1);
        assert_eq!(
            plan.per_relay["wss://a"].sub_shapes[0].shape.authors,
            ["alice".to_string()].into_iter().collect::<BTreeSet<_>>()
        );
        // Hash should not have changed (author set unchanged).
        assert_eq!(
            plan.per_relay["wss://a"].sub_shapes[0].canonical_filter_hash,
            before_hash,
            "hash must not be recomputed when authors are unchanged",
        );
    }

    #[test]
    fn one_shared_relay_for_many_authors_picks_one() {
        // 100 authors, all declaring wss://hub.
        let authors: Vec<String> = (0..100).map(|i| format!("author_{:02}", i)).collect();
        let author_refs: Vec<&str> = authors.iter().map(|s| s.as_str()).collect();
        let mut plan = plan_with(&[("wss://hub", &author_refs)]);
        apply_selection(&mut plan, 30, 2);
        assert_eq!(plan.per_relay.len(), 1);
        assert_eq!(
            plan.per_relay["wss://hub"].sub_shapes[0].shape.authors.len(),
            100
        );
    }

    #[test]
    fn disjoint_relays_all_survive_up_to_max() {
        // 5 authors, 5 disjoint relays — all survive when max_connections >= 5.
        let mut plan = plan_with(&[
            ("wss://a", &["a1"]),
            ("wss://b", &["a2"]),
            ("wss://c", &["a3"]),
            ("wss://d", &["a4"]),
            ("wss://e", &["a5"]),
        ]);
        apply_selection(&mut plan, 30, 2);
        assert_eq!(plan.per_relay.len(), 5);

        // Now clamp to 3 — exactly 3 survive.
        let mut plan2 = plan_with(&[
            ("wss://a", &["a1"]),
            ("wss://b", &["a2"]),
            ("wss://c", &["a3"]),
            ("wss://d", &["a4"]),
            ("wss://e", &["a5"]),
        ]);
        apply_selection(&mut plan2, 3, 2);
        assert_eq!(plan2.per_relay.len(), 3);
    }

    #[test]
    fn max_per_user_one_is_lte_max_per_user_two() {
        // Every pair (author, relay) is independent — but with max_per_user=1
        // each author retires after 1 hit, so fewer relays end up needed.
        let relays_input: Vec<(&str, &[&str])> = vec![
            ("wss://a", &["x", "y", "z"]),
            ("wss://b", &["x", "y", "z"]),
            ("wss://c", &["x", "y", "z"]),
        ];

        let mut p1 = plan_with(&relays_input);
        apply_selection(&mut p1, 30, 1);

        let mut p2 = plan_with(&relays_input);
        apply_selection(&mut p2, 30, 2);

        assert!(
            p1.per_relay.len() <= p2.per_relay.len(),
            "max_per_user=1 ({}) must be ≤ max_per_user=2 ({})",
            p1.per_relay.len(),
            p2.per_relay.len(),
        );
    }

    #[test]
    fn deterministic_across_runs() {
        // Build the same plan twice and confirm identical output (including
        // ordering, since BTreeMap iterates by key).
        let relays_input: Vec<(&str, &[&str])> = vec![
            ("wss://alpha", &["a", "b", "c"]),
            ("wss://beta", &["b", "c", "d"]),
            ("wss://gamma", &["c", "d", "e"]),
            ("wss://delta", &["a", "e", "f"]),
        ];

        let mut p1 = plan_with(&relays_input);
        apply_selection(&mut p1, 2, 1);
        let mut p2 = plan_with(&relays_input);
        apply_selection(&mut p2, 2, 1);

        let keys_1: Vec<_> = p1.per_relay.keys().cloned().collect();
        let keys_2: Vec<_> = p2.per_relay.keys().cloned().collect();
        assert_eq!(keys_1, keys_2, "selected relay keys must be identical across runs");
        for k in &keys_1 {
            let a1 = &p1.per_relay[k].sub_shapes[0].shape.authors;
            let a2 = &p2.per_relay[k].sub_shapes[0].shape.authors;
            assert_eq!(a1, a2, "author set on {} must be identical across runs", k);
        }
    }

    #[test]
    fn post_increment_bug_regression() {
        // 3 authors, each declaring 3 shared relays, max_per_user=2.
        //
        // Applesauce's TS source has a post-increment bug
        // (https://github.com/hzrd149/applesauce/blob/master/packages/core/src/helpers/relay-selection.ts
        // line 73: `count++` runs after `map.set`, so the stored value is
        // permanently 0 — authors with `maxRelaysPerUser >= 2` never retire,
        // and the algorithm picks every relay in the input set.
        //
        // The correct behaviour: each author gets exactly 2 relays covering
        // them, so the algorithm only needs 2 relays (each covers all 3
        // authors).
        let mut plan = plan_with(&[
            ("wss://r1", &["a", "b", "c"]),
            ("wss://r2", &["a", "b", "c"]),
            ("wss://r3", &["a", "b", "c"]),
        ]);
        apply_selection(&mut plan, 30, 2);

        assert_eq!(
            plan.per_relay.len(),
            2,
            "max_per_user=2 + 3 fully-overlapping relays must select exactly 2",
        );

        // Each author must appear on exactly 2 relays.
        let mut author_hits: HashMap<String, usize> = HashMap::new();
        for relay_plan in plan.per_relay.values() {
            for sub in &relay_plan.sub_shapes {
                for a in &sub.shape.authors {
                    *author_hits.entry(a.clone()).or_insert(0) += 1;
                }
            }
        }
        for author in ["a", "b", "c"] {
            assert_eq!(
                author_hits.get(author).copied().unwrap_or(0),
                2,
                "author {} must be covered by exactly 2 relays",
                author,
            );
        }
    }

    #[test]
    fn hash_recomputed_when_authors_change() {
        // Setup: r1 covers {a, b}, r2 covers {b, c}. With max_per_user=1,
        // max_connections=2:
        //   - Round 1: coverage = {r1: 2, r2: 2}; tiebreak on URL string
        //     (reverse-lex on equal count) picks r1 — "wss://r1" sorts before
        //     "wss://r2"; reversed, r1 is the max. r1 takes {a, b}; both retire.
        //   - Round 2: only c remains in pool with {r2}. r2 takes {c}.
        // Result: r1 keeps {a, b} unchanged → hash unchanged. r2 keeps only
        // {c} (filtered from {b, c}) → hash MUST recompute.
        let mut plan = plan_with(&[
            ("wss://r1", &["a", "b"]),
            ("wss://r2", &["b", "c"]),
        ]);
        let r1_hash_before = plan.per_relay["wss://r1"].sub_shapes[0].canonical_filter_hash.clone();
        let r2_hash_before = plan.per_relay["wss://r2"].sub_shapes[0].canonical_filter_hash.clone();

        apply_selection(&mut plan, 2, 1);

        let r1 = &plan.per_relay["wss://r1"].sub_shapes[0];
        let r2 = &plan.per_relay["wss://r2"].sub_shapes[0];
        assert_eq!(
            r1.shape.authors,
            ["a".to_string(), "b".to_string()].into_iter().collect::<BTreeSet<_>>(),
            "r1 must keep {{a, b}}",
        );
        assert_eq!(
            r2.shape.authors,
            ["c".to_string()].into_iter().collect::<BTreeSet<_>>(),
            "r2 must keep only {{c}}",
        );
        assert_eq!(
            r1.canonical_filter_hash, r1_hash_before,
            "r1 hash must be unchanged (authors unchanged)",
        );
        assert_ne!(
            r2.canonical_filter_hash, r2_hash_before,
            "r2 hash must be recomputed (authors changed from {{b,c}} → {{c}})",
        );
    }

    #[test]
    fn wildcard_sub_shape_preserved_on_surviving_relay() {
        // A relay with both a non-empty author sub-shape and a wildcard
        // sub-shape (empty authors, e.g. global kind:0). The wildcard rides
        // along regardless of coverage.
        let mut plan = plan_with(&[("wss://hub", &["alice"])]);

        // Add a second wildcard sub-shape to wss://hub.
        let mut wildcard_shape = InterestShape::default();
        wildcard_shape.kinds.insert(0);
        let wildcard_hash = canonical_filter_hash(&wildcard_shape);
        plan.per_relay
            .get_mut("wss://hub")
            .unwrap()
            .sub_shapes
            .push(SubShape {
                shape: wildcard_shape,
                originating_interests: vec![],
                canonical_filter_hash: wildcard_hash.clone(),
            });

        apply_selection(&mut plan, 30, 2);

        // Hub survives; both sub-shapes preserved.
        let hub = &plan.per_relay["wss://hub"];
        assert_eq!(hub.sub_shapes.len(), 2);
        // Wildcard sub-shape's hash MUST be unchanged (it was not mutated).
        let wildcard = hub
            .sub_shapes
            .iter()
            .find(|s| s.shape.authors.is_empty())
            .expect("wildcard sub-shape must survive");
        assert_eq!(wildcard.canonical_filter_hash, wildcard_hash);
        assert!(wildcard.shape.kinds.contains(&0));
    }

    #[test]
    fn relay_with_only_wildcard_sub_shape_is_preserved_with_budget() {
        // A relay whose only sub-shape is a wildcard (empty authors) contributes
        // nothing to coverage, but tag-scoped protocol inboxes are still real
        // subscriptions. Preserve it while there is connection budget.
        let mut wildcard_shape = InterestShape::default();
        wildcard_shape.kinds.insert(0);
        let wildcard_hash = canonical_filter_hash(&wildcard_shape);
        let wildcard_sub = SubShape {
            shape: wildcard_shape,
            originating_interests: vec![],
            canonical_filter_hash: wildcard_hash,
        };

        let mut plan = plan_with(&[("wss://hub", &["alice"])]);
        let mut role_tags = BTreeSet::new();
        role_tags.insert(RoutingSource::Nip65);
        plan.per_relay.insert(
            "wss://wildcard-only".to_string(),
            RelayPlan {
                relay_url: "wss://wildcard-only".to_string(),
                role_tags,
                sub_shapes: vec![wildcard_sub],
            },
        );

        apply_selection(&mut plan, 30, 2);

        assert!(
            plan.per_relay.contains_key("wss://wildcard-only"),
            "relay with only wildcard sub-shape must survive while budget remains",
        );
        assert!(plan.per_relay.contains_key("wss://hub"));
    }

    #[test]
    fn zero_max_connections_drops_all() {
        let mut plan = plan_with(&[("wss://a", &["alice"])]);
        apply_selection(&mut plan, 0, 2);
        assert!(plan.per_relay.is_empty());
    }

    #[test]
    fn zero_max_per_user_drops_all() {
        let mut plan = plan_with(&[("wss://a", &["alice"])]);
        apply_selection(&mut plan, 30, 0);
        assert!(plan.per_relay.is_empty());
    }
}
