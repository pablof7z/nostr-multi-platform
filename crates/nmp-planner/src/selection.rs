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

#[cfg(test)]
#[path = "selection/tests.rs"]
mod tests;

