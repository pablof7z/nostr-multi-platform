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
    /// a content fallback for case_a/case_b. It survives on the compiler for
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
    /// Additive to NIP-65 for authored REQs (case_a / case_b) and unioned
    /// with `active_account_read_relays` for the no-author firehose (case_d).
    /// When an author has no NIP-65 mailbox AND no app_relays are configured,
    /// the author is reported via `CompiledPlan::unroutable_authors`.
    app_relays: &'a [RelayUrl],
}

impl<'a> SubscriptionCompiler<'a> {
    /// Construct a compiler bound to a mailbox cache and indexer set.
    ///
    /// `active_account_read_relays` and `app_relays` default to empty —
    /// callers that need them use [`Self::with_active_account_read_relays`]
    /// or [`Self::with_relays`].
    pub fn new(mailbox_cache: &'a dyn MailboxCache, indexer_relays: &'a [RelayUrl]) -> Self {
        Self {
            mailbox_cache,
            indexer_relays,
            active_account_read_relays: &[],
            app_relays: &[],
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
        }
    }

    /// Construct with the full relay context — indexer (discovery), active-
    /// account read (firehose), and operator-configured app relays.
    ///
    /// Production callers (the subscription lifecycle) use this form so
    /// app_relays land on the additive NIP-65 lane in case_a/case_b and on
    /// the union with active-account read relays in case_d.
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
                entries.into_iter().map(|entry| entry.into_shape()).collect();

            let mut sub_shapes: Vec<(InterestShape, InterestLifecycle, Vec<InterestId>)> =
                Vec::new();
            for (shape, lifecycle, entry_sources, interest_id) in shaped {
                // Merge all source lanes from this entry into role_tags.
                for src in entry_sources {
                    role_tags.insert(src);
                }
                let mut merged = false;
                for (existing_shape, existing_lifecycle, existing_ids) in sub_shapes.iter_mut() {
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
