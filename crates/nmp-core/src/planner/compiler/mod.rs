//! The subscription compiler: 4-stage pipeline from `Vec<LogicalInterest>`
//! to `CompiledPlan`.
//!
//! ## Pipeline stages
//!
//! 1. **Resolve authors → mailboxes** — consult `MailboxCache` (phase 1 stub:
//!    `EmptyMailboxCache`; real impl in `nmp-nip65`).
//! 2. **Indexer fallback** — authors with no known mailbox route to the
//!    configured indexer set.
//! 3. **Per-relay shape merge** — group by relay URL; merge compatible shapes
//!    with `lattice::merge()` (Rules 1–8). Author sets are partitioned per
//!    relay — only authors that declared a relay appear in its sub-shape.
//! 4. **Plan-id binding** — deterministic hash → stable `plan_id`.
//!
//! ## Module structure
//!
//! - `mailbox`   — `MailboxCache` trait + `MailboxSnapshot` + phase-1 impls.
//! - `plan_id`   — `CompileContext` + `compute_plan_id` (FNV-1a hash).
//! - `partition` — `RelayEntry` + `partition_interest` (Stage 1+2).
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3
//! Doctrine: D3 (outbox routing automatic), D6 (errors never cross FFI),
//!           D8 (zero per-event allocs after warmup).

mod mailbox;
mod partition;
mod plan_id;

pub use mailbox::{EmptyMailboxCache, InMemoryMailboxCache, MailboxCache, MailboxSnapshot};
pub use plan_id::CompileContext;

use std::collections::{BTreeMap, BTreeSet};

use crate::planner::{
    interest::{InterestId, InterestLifecycle, InterestShape, LogicalInterest, RelayUrl},
    lattice::{merge, MergeOutcome},
    plan::{canonical_filter_hash, CompiledPlan, PlannerError, RelayPlan, RoutingSource, SubShape},
};
use partition::{partition_interest, RelayEntry};
use plan_id::compute_plan_id;

/// Version of the merge lattice — bump when Rule semantics change.
const MERGE_LATTICE_VERSION: u8 = 1;

// ─── SubscriptionCompiler ────────────────────────────────────────────────────

/// The subscription compiler.
///
/// Holds a reference to the mailbox cache and indexer relay set. Both may be
/// updated between compilations (the compiler always reads the current state).
///
/// ## Direction table (§3.1 / §3.2)
///
/// | Interest shape          | Direction | Relay source                                     |
/// |-------------------------|-----------|--------------------------------------------------|
/// | Has `authors`           | Outbox    | author's write relays via NIP-65 (or indexer)    |
/// | Has `#p` tag values     | Inbox     | tagged pubkey's read relays (post-v1 DMs/notifs) |
/// | Has `addresses`         | Outbox    | coord.pubkey's write relays                      |
/// | No author/addr/p        | Read      | active-account read relays (hashtag firehose)    |
pub struct SubscriptionCompiler<'a> {
    mailbox_cache: &'a dyn MailboxCache,
    /// Discovery-only relay set (kind:0/3/10002 lookups).
    ///
    /// Per the routing-rules clarification (T134), the indexer set is NEVER
    /// a content fallback for `case_a/case_b`. It survives on the compiler for
    /// two reasons:
    /// 1. Case D's cold-start fallback when both `active_account_read_relays`
    ///    and `app_relays` are empty (kernel-driven discovery bootstrap).
    /// 2. So the kernel can drive discovery REQs through the same compile
    ///    surface without growing a parallel routing path. If a caller
    ///    doesn't drive discovery this way today, they pass `&[]`.
    indexer_relays: &'a [RelayUrl],
    /// Active account read relays — for no-author/no-address interests.
    /// Phase 1: empty → falls through to `app_relays`, then indexer set.
    /// Phase 2: populated from active account's kind:10002 read-relays.
    active_account_read_relays: &'a [RelayUrl],
    /// Operator-configured app relays (T134).
    ///
    /// Additive to NIP-65 for authored REQs (`case_a` / `case_b`) and unioned
    /// with `active_account_read_relays` for the no-author firehose (`case_d`).
    /// When an author has no NIP-65 mailbox AND no `app_relays` are configured,
    /// the author is reported via `CompiledPlan::unroutable_authors`.
    app_relays: &'a [RelayUrl],
    /// Cold-start bootstrap content relays (PD-033-C planner extension).
    ///
    /// The kernel populates this from `bootstrap_urls_for_role(RelayRole::Content)`
    /// — the same well-known seed it uses for the first content socket, INCLUDING
    /// the `FALLBACK_CONTENT_RELAY` cold-start default. The compiler consults
    /// this set ONLY for `OneShot + Global + event_ids`-shaped interests, so a
    /// discovery oneshot for referenced event ids always has a content landing
    /// pad before any account configuration is loaded. Cases A/B/C never touch
    /// it; Case D consults it ahead of its existing per-relay accumulation
    /// (gated on the OneShot+Global+event_ids triple) so non-bootstrap interests
    /// retain their unchanged routing.
    ///
    /// Empty in tests and in pre-PD-033-C call sites — both `new()` and
    /// `with_relays()` default it to `&[]`, so existing callers see no
    /// behavioural change.
    bootstrap_content_relays: &'a [RelayUrl],
    /// Cold-start bootstrap indexer relays (PD-033-C planner extension).
    ///
    /// The kernel populates this from `bootstrap_urls_for_role(RelayRole::Indexer)`
    /// — the WITH-FALLBACK form (`FALLBACK_INDEXER_RELAY` when no indexer row is
    /// configured yet). This is intentionally distinct from `indexer_relays`,
    /// which is the raw (no-fallback) editable indexer set used by the
    /// mailbox-probe path and Case D's cold-start fallback. Case A's PD-033-C
    /// `if !landed && is_discovery_oneshot` arm consults THIS field instead so
    /// the planner mirrors `kernel/discovery.rs::drain_unknown_oneshots`'s
    /// profile-oneshot fan-out to `RelayRole::Indexer` exactly — cold-start
    /// included.
    ///
    /// Empty in tests and pre-PD-033-C call sites; production
    /// (`identity_state::set_relay_edit_rows`) always sets it.
    bootstrap_indexer_relays: &'a [RelayUrl],
}

impl<'a> SubscriptionCompiler<'a> {
    /// Construct a compiler bound to a mailbox cache and indexer set.
    ///
    /// `active_account_read_relays` and `app_relays` default to empty —
    /// callers that need them use [`Self::with_active_account_read_relays`]
    /// or [`Self::with_relays`].
    #[must_use]
    pub fn new(mailbox_cache: &'a dyn MailboxCache, indexer_relays: &'a [RelayUrl]) -> Self {
        Self {
            mailbox_cache,
            indexer_relays,
            active_account_read_relays: &[],
            app_relays: &[],
            bootstrap_content_relays: &[],
            bootstrap_indexer_relays: &[],
        }
    }

    /// Construct with explicit active-account read relays.
    ///
    /// `app_relays` defaults to empty — callers that need to specify app
    /// relays use [`Self::with_relays`].
    ///
    /// When `active_account_read_relays` is non-empty, no-author interests
    /// (hashtag firehose, global search) route to those relays unioned with
    /// `app_relays`, using `RoutingSource::UserConfigured(AccountRead)`
    /// and `RoutingSource::UserConfigured(AppRelay)` respectively.
    #[must_use]
    pub fn with_active_account_read_relays(
        mailbox_cache: &'a dyn MailboxCache,
        indexer_relays: &'a [RelayUrl],
        active_account_read_relays: &'a [RelayUrl],
    ) -> Self {
        Self {
            mailbox_cache,
            indexer_relays,
            active_account_read_relays,
            app_relays: &[],
            bootstrap_content_relays: &[],
            bootstrap_indexer_relays: &[],
        }
    }

    /// Construct with the full relay context — indexer (discovery), active-
    /// account read (firehose), and operator-configured app relays.
    ///
    /// Production callers (the subscription lifecycle) use this form so
    /// `app_relays` land on the additive NIP-65 lane in `case_a/case_b` and on
    /// the union with active-account read relays in `case_d`.
    #[must_use]
    pub fn with_relays(
        mailbox_cache: &'a dyn MailboxCache,
        indexer_relays: &'a [RelayUrl],
        active_account_read_relays: &'a [RelayUrl],
        app_relays: &'a [RelayUrl],
    ) -> Self {
        Self {
            mailbox_cache,
            indexer_relays,
            active_account_read_relays,
            app_relays,
            bootstrap_content_relays: &[],
            bootstrap_indexer_relays: &[],
        }
    }

    /// PD-033-C planner extension constructor — adds `bootstrap_content_relays`
    /// AND `bootstrap_indexer_relays` to the full relay context.
    ///
    /// Used by `SubscriptionLifecycle::recompile_and_diff` so the discovery
    /// oneshots from `kernel/discovery.rs::drain_unknown_oneshots` always have
    /// the correct landing pad:
    ///
    /// * `OneShot + Global + event_ids` (the events-oneshot arm) routes to
    ///   `bootstrap_content_relays` via Case D's head check — kernel-equivalent
    ///   of `RelayRole::Content` cold-start fan-out.
    /// * `OneShot + Global + authors` with no NIP-65 mailbox (the
    ///   profile-oneshot arm) routes to `bootstrap_indexer_relays` via Case A's
    ///   `if !landed` fallback — kernel-equivalent of `RelayRole::Indexer` cold-
    ///   start fan-out (carries `FALLBACK_INDEXER_RELAY` when no row is
    ///   configured yet, whereas raw `indexer_relays` would be empty).
    ///
    /// Without this constructor the partition's Case D would route the
    /// event-ids discovery REQ to `indexer_relays` (wrong — content belongs on
    /// the content lane), and Case A would mark a `OneShot + Global + authors`
    /// fetch `unroutable` (silent loss). See
    /// `docs/architecture-audit/pd033c-plan.md` §4.3.
    ///
    /// Both new fields are EXCLUDED from `compute_plan_id` so runtime toggles
    /// do not churn sub-ids — matching the `app_relays` treatment in
    /// `compile_with_context`'s Stage 4 comment.
    #[allow(clippy::too_many_arguments)]
    pub fn with_relays_and_bootstrap(
        mailbox_cache: &'a dyn MailboxCache,
        indexer_relays: &'a [RelayUrl],
        active_account_read_relays: &'a [RelayUrl],
        app_relays: &'a [RelayUrl],
        bootstrap_content_relays: &'a [RelayUrl],
        bootstrap_indexer_relays: &'a [RelayUrl],
    ) -> Self {
        Self {
            mailbox_cache,
            indexer_relays,
            active_account_read_relays,
            app_relays,
            bootstrap_content_relays,
            bootstrap_indexer_relays,
        }
    }

    /// Compile a set of logical interests into a `CompiledPlan`.
    ///
    /// **Warning — use only in tests or when policy versions are immutable.**
    ///
    /// Delegates to `compile_with_context(..., &CompileContext::default())`,
    /// which sets `indexer_set_version = 0` and `user_config_version = 0`.
    /// If the indexer relay set or active-account config changes at runtime,
    /// the resulting `plan_id` will NOT change — callers that rely on plan-id
    /// stability for subscription diffing MUST use `compile_with_context` with
    /// their own monotonic version counters.
    ///
    /// Production callers: use `compile_with_context`.
    /// Test-only / static-config callers: `compile` is safe.
    #[must_use]
    pub fn compile(
        &self,
        interests: &[LogicalInterest],
    ) -> Result<CompiledPlan, PlannerError> {
        self.compile_with_context(interests, &CompileContext::default())
    }

    /// Compile with explicit versioning context for plan-id stability.
    ///
    /// Callers that track `indexer_set_version` / `user_config_version` should
    /// use this form so plan-ids invalidate correctly on policy changes.
    #[must_use]
    pub fn compile_with_context(
        &self,
        interests: &[LogicalInterest],
        ctx: &CompileContext,
    ) -> Result<CompiledPlan, PlannerError> {
        // ── Stages 1 + 2: author-partitioned relay entry collection ──────────
        let mut relay_entries: BTreeMap<RelayUrl, Vec<RelayEntry>> = BTreeMap::new();
        // Authors that ended up with zero relay entries (no NIP-65 mailbox
        // AND no app_relays configured) are collected here so the kernel
        // can surface a UI diagnostic. Derived state — NOT part of `plan_id`
        // hashing (see `plan::CompiledPlan::unroutable_authors`).
        let mut unroutable_authors: BTreeSet<crate::planner::interest::Pubkey> = BTreeSet::new();
        for interest in interests {
            partition_interest(
                interest,
                self.mailbox_cache,
                self.indexer_relays,
                self.active_account_read_relays,
                self.app_relays,
                self.bootstrap_content_relays,
                self.bootstrap_indexer_relays,
                &mut relay_entries,
                &mut unroutable_authors,
            );
        }

        // ── Stage 3: Per-relay shape merge ──────────────────────────────────
        //
        // `role_tags` accumulates ALL RoutingSource lanes across all entries
        // for a relay, preserving the four-lane discipline (§3.1).
        let mut per_relay: BTreeMap<RelayUrl, RelayPlan> = BTreeMap::new();
        for (relay_url, entries) in relay_entries {
            let mut role_tags: BTreeSet<RoutingSource> = BTreeSet::new();
            // Shape + lifecycle + all source lanes + originating interest id.
            let shaped: Vec<(InterestShape, InterestLifecycle, BTreeSet<RoutingSource>, InterestId)> =
                entries.into_iter().map(partition::RelayEntry::into_shape).collect();

            let mut sub_shapes: Vec<(InterestShape, InterestLifecycle, Vec<InterestId>)> =
                Vec::new();
            for (shape, lifecycle, entry_sources, interest_id) in shaped {
                // Merge all source lanes from this entry into role_tags.
                for src in entry_sources {
                    role_tags.insert(src);
                }
                let mut merged = false;
                for (existing_shape, existing_lifecycle, existing_ids) in &mut sub_shapes {
                    if let MergeOutcome::Merged(new_shape) =
                        merge(&existing_shape.clone(), &shape, existing_lifecycle, &lifecycle)
                    {
                        *existing_shape = new_shape;
                        // Dedupe: the same interest_id can land on a relay more
                        // than once (e.g. when Case A's outbox push and the
                        // "both populated" inbox push both target the same
                        // relay because the author's write relay == a tagged
                        // pubkey's read relay). `originating_interests` is a
                        // set semantically, not a multiset.
                        if !existing_ids.contains(&interest_id) {
                            existing_ids.push(interest_id.clone());
                        }
                        merged = true;
                        break;
                    }
                }
                if !merged {
                    sub_shapes.push((shape, lifecycle, vec![interest_id]));
                }
            }

            let relay_sub_shapes: Vec<SubShape> = sub_shapes
                .into_iter()
                .map(|(shape, _lifecycle, ids)| {
                    let hash = canonical_filter_hash(&shape);
                    SubShape { shape, originating_interests: ids, canonical_filter_hash: hash }
                })
                .collect();

            per_relay.insert(
                relay_url.clone(),
                RelayPlan { relay_url, role_tags, sub_shapes: relay_sub_shapes },
            );
        }

        // ── Stage 4: Plan-id binding ──────────────────────────────────────────
        //
        // `unroutable_authors` is intentionally NOT fed into `compute_plan_id`
        // — it is derived state. Mailbox snapshots already feed the plan-id
        // hash via `compute_plan_id`, so a NIP-65 arrival that moves an author
        // out of the unroutable set will invalidate the plan-id correctly
        // without us having to mix the unroutable set itself into the hash.
        // App-relays are likewise excluded so the kernel can toggle them at
        // runtime without churning sub-ids.
        let plan_id = compute_plan_id(interests, self.mailbox_cache, ctx, MERGE_LATTICE_VERSION);
        Ok(CompiledPlan {
            plan_id,
            per_relay,
            unroutable_authors,
        })
    }
}

// ─── Canonical filter hash ────────────────────────────────────────────────────
//
// The canonical hash function moved to `plan::canonical_filter_hash` so any
// post-compile pass that mutates a `SubShape` can recompute its
// `canonical_filter_hash` without having to import a compiler-private helper.
// See `plan::canonical_filter_hash` for the BLAKE3-CBOR migration target.

// ─── Tests ───────────────────────────────────────────────────────────────────
//
// These tests target the `compile_with_context` ORCHESTRATION — the Stage 3
// per-relay merge and the Stage 4 plan-id binding. Routing/lane behaviour
// (Stages 1+2) is covered by the `partition::case_*` test modules; here we
// verify what those tests cannot reach: how shaped relay-entries collapse
// into `SubShape`s, how `originating_interests` accumulates and dedupes, and
// the `compile()` vs `compile_with_context` plan-id contract.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::compiler::mailbox::{InMemoryMailboxCache, MailboxSnapshot};
    use crate::planner::interest::{
        InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest, NaddrCoord,
    };
    use std::collections::BTreeSet;

    /// Deterministic 64-char hex pubkey fixture from a short label.
    fn pk(label: &str) -> String {
        format!("{label:0>64}").chars().take(64).collect()
    }

    /// A NIP-65 snapshot whose write relays are the given URLs.
    fn write_snapshot(write: &[&str]) -> MailboxSnapshot {
        MailboxSnapshot {
            write_relays: write.iter().map(|s| s.to_string()).collect(),
            read_relays: vec![],
            both_relays: vec![],
        }
    }

    /// A tailing author+kind interest. `kinds` lets callers force a merge
    /// refusal (Rule 1) by giving two interests different kind sets.
    fn author_interest(
        id: u64,
        authors: &[&str],
        kinds: &[u32],
        lifecycle: InterestLifecycle,
    ) -> LogicalInterest {
        LogicalInterest {
            id: InterestId(id),
            scope: InterestScope::Global,
            shape: InterestShape {
                authors: authors.iter().map(|a| pk(a)).collect(),
                kinds: kinds.iter().copied().collect(),
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle,
        }
    }

    // ── Gap 1: empty interests → empty plan ─────────────────────────────────

    /// An empty interest slice compiles to an empty plan — no `per_relay`
    /// entries, no `unroutable_authors`, no panic, and an `Ok` result. The
    /// `PlannerError::EmptyInterestSet` variant is defensive-only: an empty
    /// input is a valid (empty) plan, NOT an error (see `plan::PlannerError`).
    #[test]
    fn empty_interests_compile_to_empty_plan() {
        let cache = InMemoryMailboxCache::new();
        let compiler = SubscriptionCompiler::new(&cache, &[]);

        let plan = compiler.compile(&[]).expect("empty input is Ok, not an error");

        assert!(plan.per_relay.is_empty(), "no relays for an empty interest set");
        assert!(
            plan.unroutable_authors.is_empty(),
            "no authors, so nothing can be unroutable"
        );
        assert!(!plan.plan_id.is_empty(), "even the empty plan carries a plan-id");
    }

    /// The empty-input plan-id is deterministic across recompiles — the
    /// idempotency check the wire-emitter diff relies on still holds at zero
    /// interests.
    #[test]
    fn empty_interests_plan_id_is_deterministic() {
        let cache = InMemoryMailboxCache::new();
        let compiler = SubscriptionCompiler::new(&cache, &[]);

        let first = compiler.compile(&[]).expect("compile");
        let second = compiler.compile(&[]).expect("compile");
        assert_eq!(
            first.plan_id, second.plan_id,
            "two compiles of an empty interest set must share a plan-id"
        );
    }

    // ── Gap 2: single author interest → correct filter shape ────────────────

    /// One author with a known NIP-65 write relay produces exactly one
    /// `RelayPlan` carrying exactly one `SubShape`, whose shape echoes the
    /// interest's authors+kinds and names the originating interest.
    #[test]
    fn single_author_interest_produces_one_subshape() {
        let mut cache = InMemoryMailboxCache::new();
        cache.put(pk("alice"), write_snapshot(&["wss://alice-write"]));
        let compiler = SubscriptionCompiler::new(&cache, &[]);

        let plan = compiler
            .compile(&[author_interest(1, &["alice"], &[1], InterestLifecycle::Tailing)])
            .expect("compile");

        assert_eq!(plan.per_relay.len(), 1, "exactly one relay in the plan");
        let relay = plan.per_relay.get("wss://alice-write").expect("alice-write relay");
        assert_eq!(relay.sub_shapes.len(), 1, "one interest → one sub-shape");

        let sub = &relay.sub_shapes[0];
        // Author-partitioning: the sub-shape's author set is exactly Alice.
        assert_eq!(sub.shape.authors, [pk("alice")].into_iter().collect::<BTreeSet<_>>());
        assert_eq!(sub.shape.kinds, [1u32].into_iter().collect::<BTreeSet<_>>());
        // Provenance: the sub-shape names interest #1.
        assert_eq!(sub.originating_interests, vec![InterestId(1)]);
        // The cached hash matches a fresh hash of the shape.
        assert_eq!(sub.canonical_filter_hash, canonical_filter_hash(&sub.shape));
    }

    // ── Gap 3: two compatible interests for the same relay → merged ─────────

    /// Two interests with mergeable shapes (same kinds, same lifecycle) that
    /// route to the SAME relay collapse into a single `SubShape`. Stage 3's
    /// greedy merge unions the author sets and records BOTH originating
    /// interest ids on the one sub-shape.
    #[test]
    fn two_compatible_interests_same_relay_merge_into_one_subshape() {
        let mut cache = InMemoryMailboxCache::new();
        // Two distinct authors, both publishing to the same write relay.
        cache.put(pk("alice"), write_snapshot(&["wss://shared"]));
        cache.put(pk("bob"), write_snapshot(&["wss://shared"]));
        let compiler = SubscriptionCompiler::new(&cache, &[]);

        let plan = compiler
            .compile(&[
                author_interest(1, &["alice"], &[1], InterestLifecycle::Tailing),
                author_interest(2, &["bob"], &[1], InterestLifecycle::Tailing),
            ])
            .expect("compile");

        let relay = plan.per_relay.get("wss://shared").expect("shared relay");
        assert_eq!(
            relay.sub_shapes.len(),
            1,
            "two mergeable interests on one relay collapse into one REQ"
        );
        let sub = &relay.sub_shapes[0];
        // Merged shape unions both authors.
        assert_eq!(
            sub.shape.authors,
            [pk("alice"), pk("bob")].into_iter().collect::<BTreeSet<_>>()
        );
        // Both interest ids are recorded on the merged sub-shape.
        let ids: BTreeSet<InterestId> = sub.originating_interests.iter().cloned().collect();
        assert_eq!(ids, [InterestId(1), InterestId(2)].into_iter().collect());
    }

    // ── Gap 3 (refusal): two incompatible interests → two sub-shapes ────────

    /// Two interests that route to the same relay but FAIL the merge lattice
    /// (here Rule 1 — different kind sets) produce TWO distinct `SubShape`s
    /// on the one `RelayPlan`: one wire REQ each.
    #[test]
    fn incompatible_kinds_same_relay_stay_distinct_subshapes() {
        let mut cache = InMemoryMailboxCache::new();
        cache.put(pk("alice"), write_snapshot(&["wss://shared"]));
        cache.put(pk("bob"), write_snapshot(&["wss://shared"]));
        let compiler = SubscriptionCompiler::new(&cache, &[]);

        let plan = compiler
            .compile(&[
                // kind:1 — text notes.
                author_interest(1, &["alice"], &[1], InterestLifecycle::Tailing),
                // kind:30023 — long-form. Rule 1 refuses (distinct, no wildcard).
                author_interest(2, &["bob"], &[30023], InterestLifecycle::Tailing),
            ])
            .expect("compile");

        let relay = plan.per_relay.get("wss://shared").expect("shared relay");
        assert_eq!(
            relay.sub_shapes.len(),
            2,
            "incompatible kind sets must NOT merge — two REQs on the relay"
        );
    }

    /// Two interests on the same relay with different LIFECYCLES (Tailing vs
    /// OneShot) fail Rule 6 and stay as two `SubShape`s — the wire-emitter
    /// needs distinct frames so it can CLOSE the one-shot REQ on EOSE while
    /// leaving the tailing one open.
    #[test]
    fn mixed_lifecycle_same_relay_stays_distinct_subshapes() {
        let mut cache = InMemoryMailboxCache::new();
        cache.put(pk("alice"), write_snapshot(&["wss://shared"]));
        cache.put(pk("bob"), write_snapshot(&["wss://shared"]));
        let compiler = SubscriptionCompiler::new(&cache, &[]);

        let plan = compiler
            .compile(&[
                author_interest(1, &["alice"], &[1], InterestLifecycle::Tailing),
                author_interest(2, &["bob"], &[1], InterestLifecycle::OneShot),
            ])
            .expect("compile");

        let relay = plan.per_relay.get("wss://shared").expect("shared relay");
        assert_eq!(
            relay.sub_shapes.len(),
            2,
            "Rule 6 refuses cross-lifecycle merges — two REQs on the relay"
        );
    }

    // ── Gap 4: originating_interests dedup ──────────────────────────────────

    /// An interest with explicit `authors` AND `#p` tag values fires both the
    /// Case A outbox push and the "both populated" inbox push. When the
    /// author's write relay and the tagged pubkey's read relay are the SAME
    /// URL, the one interest_id lands on that relay twice — Stage 3 must
    /// record it only once (`originating_interests` is a set, not a multiset).
    #[test]
    fn same_interest_on_one_relay_via_two_lanes_dedupes_originating_id() {
        let mut cache = InMemoryMailboxCache::new();
        // Alice (the author) writes to wss://shared.
        cache.put(pk("alice"), write_snapshot(&["wss://shared"]));
        // Carol (the #p-tagged recipient) READS from the very same wss://shared.
        cache.put(
            pk("carol"),
            MailboxSnapshot {
                write_relays: vec![],
                read_relays: vec!["wss://shared".to_string()],
                both_relays: vec![],
            },
        );
        let compiler = SubscriptionCompiler::new(&cache, &[]);

        // One interest: author Alice + #p:[Carol].
        let mut tags = std::collections::BTreeMap::new();
        tags.insert("p".to_string(), [pk("carol")].into_iter().collect::<BTreeSet<_>>());
        let interest = LogicalInterest {
            id: InterestId(1),
            scope: InterestScope::Global,
            shape: InterestShape {
                authors: [pk("alice")].into_iter().collect(),
                kinds: [1u32].into_iter().collect(),
                tags,
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle: InterestLifecycle::Tailing,
        };

        let plan = compiler.compile(&[interest]).expect("compile");
        let relay = plan.per_relay.get("wss://shared").expect("shared relay");

        // Across ALL sub-shapes on the relay, interest #1 must appear exactly
        // once per sub-shape's originating list — never duplicated.
        for sub in &relay.sub_shapes {
            let count = sub
                .originating_interests
                .iter()
                .filter(|id| **id == InterestId(1))
                .count();
            assert!(
                count <= 1,
                "interest id must be deduped within a sub-shape; saw it {count} times"
            );
        }
    }

    // ── Gap 5: role_tags accumulation across distinct interests ─────────────

    /// One relay reached by two different interests via two different lanes
    /// (author A via NIP-65, author B via AppRelay because the operator
    /// pinned the same URL) must carry BOTH lanes in `role_tags` — the
    /// four-lane discipline is preserved across interest boundaries, not just
    /// within one interest.
    #[test]
    fn role_tags_accumulate_across_interests_on_a_shared_relay() {
        let mut cache = InMemoryMailboxCache::new();
        // Alice declares wss://shared as her NIP-65 write relay.
        cache.put(pk("alice"), write_snapshot(&["wss://shared"]));
        // Bob has no mailbox; he will only ride the app-relay lane.
        let app = vec!["wss://shared".to_string()];
        let compiler = SubscriptionCompiler::with_relays(&cache, &[], &[], &app);

        let plan = compiler
            .compile(&[
                author_interest(1, &["alice"], &[1], InterestLifecycle::Tailing),
                author_interest(2, &["bob"], &[1], InterestLifecycle::Tailing),
            ])
            .expect("compile");

        let relay = plan.per_relay.get("wss://shared").expect("shared relay");
        assert!(
            relay.role_tags.contains(&RoutingSource::Nip65),
            "Alice's NIP-65 lane must be recorded"
        );
        assert!(
            relay.role_tags.contains(&RoutingSource::UserConfigured(
                crate::planner::plan::UserConfiguredCategory::AppRelay
            )),
            "Bob's AppRelay lane must be recorded on the same relay"
        );
    }

    // ── Gap 6: compile() vs compile_with_context() plan-id contract ─────────

    /// `compile()` pins the `CompileContext` to its default (both version
    /// counters at 0). Two `compile_with_context` calls with DIFFERENT
    /// contexts must produce different plan-ids for the same interests — the
    /// stability contract the doc-comment on `compile()` warns about.
    #[test]
    fn compile_with_context_plan_id_tracks_the_context() {
        let mut cache = InMemoryMailboxCache::new();
        cache.put(pk("alice"), write_snapshot(&["wss://alice-write"]));
        let compiler = SubscriptionCompiler::new(&cache, &[]);
        let interests = [author_interest(1, &["alice"], &[1], InterestLifecycle::Tailing)];

        let v0 = compiler
            .compile_with_context(&interests, &CompileContext::default())
            .expect("compile");
        let v1 = compiler
            .compile_with_context(
                &interests,
                &CompileContext { indexer_set_version: 0, user_config_version: 1 },
            )
            .expect("compile");

        assert_ne!(
            v0.plan_id, v1.plan_id,
            "a bumped user_config_version must change the plan-id"
        );
        // `compile()` is exactly `compile_with_context(.., &default())`.
        let via_default = compiler.compile(&interests).expect("compile");
        assert_eq!(
            v0.plan_id, via_default.plan_id,
            "compile() must equal compile_with_context with a default context"
        );
    }

    // ── Gap 7: unroutable_authors is excluded from plan_id ──────────────────

    /// Toggling `app_relays` flips an author between routable and unroutable,
    /// but `app_relays` is deliberately NOT fed into `compute_plan_id`. So a
    /// compile WITH app-relays and one WITHOUT — same interests, same mailbox
    /// cache, same context — must share a plan-id even though their
    /// `unroutable_authors` sets differ. (The wire-emitter diff must not
    /// churn sub-ids when the operator toggles app relays at runtime.)
    #[test]
    fn app_relay_toggle_changes_unroutable_set_but_not_plan_id() {
        // Bob has no NIP-65 mailbox — his routability depends entirely on
        // whether app_relays are configured.
        let cache = InMemoryMailboxCache::new();
        let interests = [author_interest(1, &["bob"], &[1], InterestLifecycle::Tailing)];

        // Without app relays: Bob is unroutable.
        let no_app = SubscriptionCompiler::new(&cache, &[]);
        let plan_no_app = no_app.compile(&interests).expect("compile");
        assert!(
            plan_no_app.unroutable_authors.contains(&pk("bob")),
            "with no app relays Bob must be unroutable"
        );

        // With app relays: Bob is routable.
        let app = vec!["wss://app".to_string()];
        let with_app = SubscriptionCompiler::with_relays(&cache, &[], &[], &app);
        let plan_with_app = with_app.compile(&interests).expect("compile");
        assert!(
            plan_with_app.unroutable_authors.is_empty(),
            "with app relays configured Bob must be routable"
        );

        // The two plans differ in their unroutable set...
        assert_ne!(
            plan_no_app.unroutable_authors, plan_with_app.unroutable_authors,
            "the unroutable set genuinely differs between the two compiles"
        );
        // ...but the plan-id is identical — app_relays are excluded from the hash.
        assert_eq!(
            plan_no_app.plan_id, plan_with_app.plan_id,
            "toggling app_relays must not perturb the plan-id (it is excluded \
             from compute_plan_id — see Stage 4 comment in compile_with_context)"
        );
    }

    /// Counterpart to the app-relay-toggle test: a NIP-65 mailbox ARRIVAL for
    /// the same author DOES change the plan-id. The mailbox snapshot for
    /// referenced pubkeys feeds `compute_plan_id`, so moving an author out of
    /// the unroutable set via NIP-65 (rather than via app-relays) correctly
    /// invalidates the plan.
    #[test]
    fn nip65_arrival_changes_plan_id_even_via_unroutable_author() {
        let interests = [author_interest(1, &["bob"], &[1], InterestLifecycle::Tailing)];

        // Before NIP-65: empty cache, Bob unroutable.
        let empty_cache = InMemoryMailboxCache::new();
        let before = SubscriptionCompiler::new(&empty_cache, &[])
            .compile(&interests)
            .expect("compile");
        assert!(before.unroutable_authors.contains(&pk("bob")));

        // After NIP-65: Bob's kind:10002 arrives in the cache.
        let mut cache_with_bob = InMemoryMailboxCache::new();
        cache_with_bob.put(pk("bob"), write_snapshot(&["wss://bob-write"]));
        let after = SubscriptionCompiler::new(&cache_with_bob, &[])
            .compile(&interests)
            .expect("compile");
        assert!(after.unroutable_authors.is_empty());

        assert_ne!(
            before.plan_id, after.plan_id,
            "a NIP-65 mailbox arrival for a referenced author must change the plan-id"
        );
    }

    // ── Mixed-shape interests on one relay (timeline + profile) ─────────────

    /// A timeline interest (kinds {1,6}, no limit) and a profile interest
    /// (kinds {0,3,10002}, limit Some(3)) for the SAME author route to the
    /// same write relay but cannot merge — different kinds (Rule 1) and a
    /// limit on one side (Rule 5). The relay therefore carries two distinct
    /// sub-shapes, each with the correct filter shape.
    #[test]
    fn timeline_and_profile_for_same_author_produce_two_subshapes() {
        let mut cache = InMemoryMailboxCache::new();
        cache.put(pk("alice"), write_snapshot(&["wss://alice-write"]));
        let compiler = SubscriptionCompiler::new(&cache, &[]);

        let timeline = LogicalInterest {
            id: InterestId(1),
            scope: InterestScope::Global,
            shape: InterestShape::timeline_for([pk("alice")].into_iter().collect()),
            hints: Vec::new(),
            lifecycle: InterestLifecycle::Tailing,
        };
        let profile = LogicalInterest {
            id: InterestId(2),
            scope: InterestScope::Global,
            shape: InterestShape::profile_for(pk("alice")),
            hints: Vec::new(),
            lifecycle: InterestLifecycle::OneShot,
        };

        let plan = compiler.compile(&[timeline, profile]).expect("compile");
        let relay = plan.per_relay.get("wss://alice-write").expect("alice-write relay");
        assert_eq!(
            relay.sub_shapes.len(),
            2,
            "timeline and profile shapes cannot merge — two REQs on the relay"
        );

        // Exactly one sub-shape carries the timeline kinds, one the profile kinds.
        let timeline_kinds: BTreeSet<u32> = [1, 6].into_iter().collect();
        let profile_kinds: BTreeSet<u32> = [0, 3, 10002].into_iter().collect();
        let has_timeline = relay.sub_shapes.iter().any(|s| s.shape.kinds == timeline_kinds);
        let has_profile = relay.sub_shapes.iter().any(|s| s.shape.kinds == profile_kinds);
        assert!(has_timeline, "one sub-shape must carry the timeline kinds {{1,6}}");
        assert!(has_profile, "one sub-shape must carry the profile kinds {{0,3,10002}}");

        // The profile sub-shape preserves its limit (Rule 5 would have refused
        // any merge that dropped it).
        let profile_sub = relay
            .sub_shapes
            .iter()
            .find(|s| s.shape.kinds == profile_kinds)
            .expect("profile sub-shape");
        assert_eq!(profile_sub.shape.limit, Some(3), "profile limit must survive");
    }

    /// A naddr-coordinate address pointer (Case B) routes to the addressed
    /// author's write relay and produces a sub-shape whose `addresses` field
    /// carries the coordinate verbatim.
    #[test]
    fn address_pointer_interest_routes_coord_to_authors_write_relay() {
        let mut cache = InMemoryMailboxCache::new();
        cache.put(pk("author"), write_snapshot(&["wss://author-write"]));
        let compiler = SubscriptionCompiler::new(&cache, &[]);

        let coord = NaddrCoord {
            pubkey: pk("author"),
            kind: 30023,
            d_tag: "long-form".to_string(),
        };
        let interest = LogicalInterest {
            id: InterestId(1),
            scope: InterestScope::Global,
            shape: InterestShape {
                kinds: [30023u32].into_iter().collect(),
                addresses: [coord.clone()].into_iter().collect(),
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle: InterestLifecycle::OneShot,
        };

        let plan = compiler.compile(&[interest]).expect("compile");
        let relay = plan.per_relay.get("wss://author-write").expect("author-write relay");
        assert_eq!(relay.sub_shapes.len(), 1, "one address pointer → one REQ");
        assert!(
            relay.sub_shapes[0].shape.addresses.contains(&coord),
            "the sub-shape must carry the naddr coordinate verbatim"
        );
    }
}
