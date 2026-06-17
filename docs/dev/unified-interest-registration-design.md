# Unified Interest Registration — Design

**Status:** Design — **codex-validated 2026-06-17; sound to implement** with the four
amendments in "§0 Codex amendments (binding)" below, which OVERRIDE the body where they
conflict. Date: 2026-06-17.
**Mandate:** ONE front-door for "register an interest in events for X", store-first
by construction (serve-from-store + relay REQ are both intrinsic to it). The legacy
`push` / `push_if_changed` (registry) and `push_interest_and_serve` (kernel) surface
is **completely deleted**, every caller migrated. No parallel registration path remains.

> **Companion doc:** The detailed current-surface enumeration (§0), complete
> caller-migration table (§3), and follow-feed batch detail (§4) live in
> [`unified-interest-registration-callers.md`](unified-interest-registration-callers.md).

## §0 Codex amendments (binding — override the body on conflict)

1. **The front-door KEEPS a batch/slice path — "no batch" was WRONG (integrated codex review, #1497 context).** Collapsing the follow-feed to one multi-author interest removes the follow-feed's N-fan-out, but it is NOT the only N-at-once registration site. The Marmot group-subscription path loops over relay-pinned interests and registers each (`crates/nmp-marmot/src/projection/state.rs:475`, built per-relay at `crates/nmp-marmot/src/interest.rs:103`). So the front-door MUST expose a batch/slice form (`register_interest(items: &[{identity, interest, policy}], reason)` → apply all, enqueue deferred, one drain) OR every remaining N-loop must be explicitly migrated/justified first. The slice with N=1 is the single-interest degenerate case (drains before the post-dispatch emit `crates/nmp-core/src/actor/dispatch.rs:452/461`, so D1 holds); N>1 (follow-feed before its collapse, Marmot) does the deferred-enqueue + single-drain. **Do not delete the batch.** Profile/NIP-29-discovery/zap/WoT are one-at-a-time and don't need it.

   **Sequencing (binding, revised):** (a) #1497 base — `StoreQuery::AuthorsKind` (ships alone); (b) #1497 higher layer — follow-feed → one multi-author interest, drop the per-author `limit` **(product/relay-load decision required — see amendment 5)**, delete `TIMELINE_AUTHOR_LIMIT`/`capped_contact_follows` **ONLY after a replacement profile-cache memory bound exists — see amendment 6**; (c) THIS doc — the front-door (with the slice/batch form), seal the raw mutators, delete the entire legacy `push` surface. C does NOT strictly depend on B; if C ships first it simply uses the slice form for the still-N follow-feed.
2. **Withdraw-by-id = token-gated `drop_slot_by_key` across ALL scopes.** The body's
   claim "all external producers are Global" is FALSE: NIP-17 giftwrap registers at
   `InterestScope::ActiveAccount` (`crates/nmp-nip17/src/inbox.rs:408/410`), and
   `legacy_scope` currently maps `ActiveAccount → Global` (`subs/registry.rs:225`). To
   preserve withdraw-by-id byte-identically, the sealed write surface must expose a
   `drop_slot_by_key` that removes the slot regardless of scope — do NOT reconstruct a
   `drop_owner` call with an assumed scope.
3. **`for_test()` seam gated.** The registry/lifecycle test seam that bypasses the
   front-door must be `#[cfg(any(test, feature = "test-support"))]`, plus a
   doctrine-lint backstop forbidding production references to the raw mutators /
   `RegistryWriteToken::for_test`.
4. **Migrate direct-registry test fixtures.** The caller table must also cover every
   test that calls `ensure_sub`/`set_sub`/`push`/`push_if_changed` directly (rg across
   `subs/*` and `nmp-testing/tests/*`); these move to the `for_test()` seam or the
   front-door. End state: ZERO production references to the legacy surface, and the only
   raw-mutator callers are the sealed `apply` + the gated test seam.
5. **Just drop the follow-feed `limit` — NOT a product decision (owner, non-problem).**
   `follow_feed_interest`'s `limit` (`FOLLOW_FEED_LIMIT = 1000`,
   `crates/nmp-core/src/kernel/ingest/contacts.rs:39/61`, emitted on the wire filter
   `wire.rs:287`) exists ONLY as the merge-blocker that forces per-author fan-out. The
   "unbounded backfill" worry is a non-problem: relays send what they choose, and the
   feed already requests windowed chunks (nmp-feed), so the per-request limit is not
   load protection. Drop it, collapse to one multi-author interest, and update the test
   that asserts `1000`. The ONE real mechanical fact (not a decision): `limit` must stay
   part of the shape-equality check used for recompile — it drives the wire REQ
   (`compiler/partition/mod.rs:169`, `wire.rs:287`), so excluding it would miss real
   wire changes.
6. **Profile cache → plain bounded LRU; delete the cap freely (owner, non-problem).**
   `TIMELINE_AUTHOR_LIMIT`/`capped_contact_follows` are deleted with no replacement
   ceremony. Today the profile-cache HWM is `2 × TIMELINE_AUTHOR_LIMIT` and PINS every
   followed author from eviction (`kernel/ram_eviction.rs:80/113/407`,
   `ram_eviction_tests.rs:290`) — that pin-all-followed invariant is overkill. Replace it
   with a straightforward bounded LRU decoupled from the follow count: evict
   least-recently-used; an evicted followed-author profile just re-serves from disk
   instantly once C makes kind:0 store-first. Update `ram_eviction.rs` + its tests to LRU,
   and remove the cap from `tags.rs:107/124`, `ingest/contacts.rs:191`,
   `nmp-nip02/src/active_follow_set.rs:336`, `nmp-nip02/src/projection.rs:220`,
   `nmp-nip02/src/cap_divergence_tests.rs`.
7. **Do NOT collapse profile claims into one multi-author interest.** The kind:0
   cold-start fix lands via C independently of #1497 (profile claims are separate from
   the follow-feed; the bug is the direct `set_sub` without cache-serve at
   `profile.rs:274`, fixed by routing through the front-door). Profile claims have
   per-pubkey owners, liveness-upgrade semantics, and per-claim relay hints
   (`profile.rs:202/234`); the planner already coalesces same-shape kind:0 claims when
   `limit: None` (`profile.rs:21`). A profile-claim `AuthorsKind` batching is a separate
   deliberate design, NOT a drop-in replacement, and is out of scope here.

Verified by codex: the `RegistryWriteToken` sealing is sound; the `changed` predicate
must be shape∨lifecycle∨hints (lifecycle-only profile CacheOk→Live and hint-only claim
expansion must recompile the REQ, while completion-key idempotency keeps the serve a
no-op) — and per amendment 5, `limit` stays IN the shape comparison (it is wire-relevant);
the `OneshotApi::request` → pure `prepare` + Kernel-driven register split is
sound; D1 timing, completion-key idempotency, account-switch clear-before-serve,
multi-owner GC, and store-first-additive (REQ always fires, incl. bootstrap/discovery)
are all preserved. Residual: decide `FollowListChanged` trigger keep/delete only after
updating its tests/docs (it's still the named recompile signal).

All claims are grounded in `file:line`. The two prior investigations
(`docs/dev/store-first-layering-investigation.md`,
`docs/dev/kind0-and-notice-investigation.md`) describe the bug class; this document
is the implementable end-state.

---

## 1 — The single registration front-door

Add to `crates/nmp-core/src/kernel/cache_serve/mod.rs` (the module that already owns the
enqueue+drain recipe and the completion-key derivation — keeping the recipe in one place,
per the PR #1237 F3 lesson, `cache_serve/mod.rs:140-150`):

```rust
/// Conflict/write policy for [`Kernel::register_interest`] — the two LEGITIMATE
/// distinct registry semantics, named.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InterestWrite {
    /// Join-if-absent. Attach the owner; install the interest ONLY if the
    /// `(scope, key)` slot is absent. A re-mount never clobbers an existing
    /// shared slot (today's `ensure_sub`). Used by: generic feed opens
    /// (`OpenInterest`/`open_interest_sub`), `open_uri`, oneshot discovery.
    EnsureAbsent,
    /// Force-replace. Attach the owner and replace the slot's interest
    /// (today's `set_sub` / legacy `push`). Used by: bootstrap self-kinds and
    /// account switch (author swap), profile-claim liveness upgrade
    /// (OneShot→Tailing), claim-expansion hint update, and the legacy
    /// `ActorCommand::PushInterest` command.
    Replace,
}

/// Outcome of a unified registration (diagnostics + caller branching).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct InterestRegistration {
    /// The `(scope, key)` slot was absent and the interest was installed.
    pub newly_installed: bool,
    /// A plan-relevant field of the registered interest changed
    /// (`EnsureAbsent` ⇒ always equals `newly_installed`; `Replace` ⇒ true
    /// when newly installed OR the stored interest differed).
    pub changed: bool,
}
```

Front-door:

```rust
/// THE single production front-door for registering an interest in events.
///
/// Store-first by construction: on a newly-installed or plan-changed
/// registration it ALWAYS (a) performs the store-cache serve
/// (`enqueue_interest_cache_serve`, which enqueues AND drains one aggregate
/// chunk synchronously so the first snapshot after install carries store data —
/// the D1 guarantee, `cache_serve/mod.rs:132`) AND (b) enqueues the recompile
/// trigger so the wire REQ (the refinement half) follows. A caller can NEVER
/// register an interest without store-serving it — the serve is not optional and
/// not the caller's job (ADR-0045 R2.1 single-mechanism; R3 store-first additive).
///
/// `policy` preserves the two legitimate semantics (see [`InterestWrite`]).
/// `reason` labels the recompile trigger for diagnostics.
pub(crate) fn register_interest(
    &mut self,
    identity: crate::subs::SubIdentity,
    interest: crate::planner::LogicalInterest,
    policy: InterestWrite,
    reason: &'static str,
) -> InterestRegistration {
    let serve_key = identity.key;
    let serve_shape = interest.shape.clone();
    let token = RegistryWriteToken::new();           // sealed mint — see §2
    let reg = self
        .lifecycle
        .registry_mut()
        .apply(&token, policy, identity, interest); // returns InterestRegistration

    if reg.changed {
        self.lifecycle
            .enqueue_trigger(crate::subs::CompileTrigger::InvalidateCompile {
                reason: crate::subs::InvalidateReason::External(reason.to_string()),
            });
        self.enqueue_interest_cache_serve(&serve_key, &serve_shape);
    }
    reg
}
```

`InterestRegistry::apply` (new, in `registry.rs`, replaces the four mutators' public
surface — see §2 for sealing and §3 for the `changed` predicate):

```rust
pub(crate) fn apply(
    &mut self,
    _t: &RegistryWriteToken,            // sealed: only the front-door can mint it
    policy: InterestWrite,
    identity: SubIdentity,
    interest: LogicalInterest,
) -> InterestRegistration {
    let shared = identity.shared();
    match policy {
        InterestWrite::EnsureAbsent => {
            let newly = !self.slots.contains_key(&shared);
            // attach owner; install only if absent (today's ensure_sub body)
            ...
            InterestRegistration { newly_installed: newly, changed: newly }
        }
        InterestWrite::Replace => {
            let changed = match self.slots.get(&shared) {
                None => true,
                Some(slot) => plan_relevant_change(&slot.interest, &interest),
            };
            let newly = !self.slots.contains_key(&shared);
            // attach owner; REPLACE the interest unconditionally (today's set_sub
            // body) so a lifecycle/hint upgrade always lands in the slot...
            ...
            InterestRegistration { newly_installed: newly, changed }
        }
    }
}
```

**Where it lives:** `crate::kernel::cache_serve` (the front-door = a `Kernel` method);
`InterestWrite` / `InterestRegistration` are re-exported `pub(in crate::kernel)` for the
dispatch/requests call sites. `InterestRegistry::apply` lives in `crate::subs::registry`.

**Why `changed` includes lifecycle/hints, not just shape (the §3 predicate):** today
`push_if_changed` gates on `shape` only, but `set_sub` callers (profile liveness upgrade,
claim-expansion hint update) rely on `set_sub` always replacing the interest AND always
recompiling. A CacheOk→Live profile upgrade changes only `lifecycle` (shape identical,
`profile.rs:243-247`); if the trigger gated on shape alone the wire REQ would stay
OneShot and CLOSE on EOSE — the Live claim would never get a tailing sub. So the unified
`changed` predicate is a plan-relevant diff (see §3 in the companion doc) that subsumes
both.

### `register_interests_batch` (the follow-feed mode)

```rust
/// Batch front-door: register N interests under ONE trailing synchronous drain.
/// Same mechanism as `register_interest`, batched — NOT a parallel path. For the
/// follow-feed sync (one interest per followed pubkey, up to ~500): enqueue each
/// serve DEFERRED (no per-author drain), enqueue ONE coalesced recompile trigger,
/// then drain ONE aggregate-budget chunk for the whole batch (ADR-0045 §5
/// anti-burst; a 300–500-follow cold start drains once, not per author).
pub(crate) fn register_interests_batch<I>(
    &mut self,
    items: I,
    policy: InterestWrite,
    reason: &'static str,
) where
    I: IntoIterator<Item = (crate::subs::SubIdentity, crate::planner::LogicalInterest)>,
{
    let token = RegistryWriteToken::new();
    let mut any_changed = false;
    for (identity, interest) in items {
        let serve_key = identity.key;
        let serve_shape = interest.shape.clone();
        let reg = self.lifecycle.registry_mut().apply(&token, policy, identity, interest);
        if reg.changed {
            any_changed = true;
            self.enqueue_interest_cache_serve_deferred(&serve_key, &serve_shape); // NO drain
        }
    }
    if any_changed {
        self.lifecycle.enqueue_trigger(crate::subs::CompileTrigger::InvalidateCompile {
            reason: crate::subs::InvalidateReason::External(reason.to_string()),
        });
        self.run_cache_serve_step(); // ONE drain for the whole batch
    }
}
```

This reuses the exact primitives `register_interest` uses
(`apply` + `enqueue_interest_cache_serve_deferred` + `run_cache_serve_step`); the only
difference is N deferred enqueues share one drain. It is the SAME mechanism, batched.

---

## 2 — Sealing the raw mutators

**Goal:** after migration, the ONLY code that can mutate the registry's interest set is
the front-door (`register_interest` / `register_interests_batch`). Production install
sites cannot call `set_sub` / `ensure_sub` / `push` / `push_if_changed`.

**Constraint (Rust module rules, verified against the tree):** `pub(in path)` requires
`path` to be an *ancestor* of the item's module. The registry lives in `crate::subs`
(`registry.rs`); the front-door lives in `crate::kernel::cache_serve` — **siblings**, so
`pub(in crate::kernel::cache_serve)` on a `crate::subs` item is illegal. A plain
`pub(crate)` is too broad (`profile.rs`, `startup.rs`, `contacts.rs` all sit inside the
crate and could still call it). Therefore the seal is a **sealed write token**:

```rust
// in crate::kernel::cache_serve (the front-door's own module)
/// Capability token proving the caller is the unified interest front-door.
/// Its field is private to this module, so ONLY `cache_serve` can mint one
/// (`register_interest` / `register_interests_batch`). `InterestRegistry::apply`
/// requires `&RegistryWriteToken`, so no other module can mutate the registry.
pub(crate) struct RegistryWriteToken {
    _seal: (),
}
impl RegistryWriteToken {
    pub(in crate::kernel::cache_serve) fn new() -> Self { Self { _seal: () } }
    #[cfg(any(test, feature = "test-registry-seam"))]
    pub fn for_test() -> Self { Self { _seal: () } } // test-only seam (below)
}
```

- The token TYPE is `pub(crate)` (so `registry.rs` can name it in `apply`'s signature),
  but the production constructor `new()` is `pub(in crate::kernel::cache_serve)` — only
  the front-door can mint it. `registry.rs` referencing a `crate::kernel` type is a
  within-crate reference (no new crate dependency; `nmp-core` is one crate).
- `InterestRegistry::ensure_sub` / `set_sub` / `push` / `push_if_changed` are **deleted**;
  their bodies fold into `apply(&RegistryWriteToken, …)`. `drop_owner`, `withdraw`'s
  replacement, `iter_active`, `iter_active_with_keys`, `owner_count`, `len`, `is_empty`
  stay (reads + un-register, not in scope of the registration seal).
- `registry_mut()` (`lifecycle.rs:378`) stays `pub` — holding `&mut InterestRegistry` is
  harmless now because every mutator needs a token. Readers (`should_store_event`'s
  `iter_active`, K3 truncated-serve `iter_active_with_keys`) are unaffected.

**Test-only seam.** Tests that build a bare `SubscriptionLifecycle`/`InterestRegistry`
(no `Kernel`) and push interests directly — `subs/registry.rs` tests, `subs/oneshot.rs`,
`subs/interest_builder.rs`, `subs/lifecycle_tests.rs`, `subs/since_rewrite_tests.rs`,
`subs/discovery_tests.rs`, `subs/coverage_hook_tests.rs`, `subs/attribution_split_tests.rs`,
and integration tests `nmp-testing/tests/e2e_full_pipeline.rs`,
`m8_subscription_lifecycle.rs`, `t142_drain_tick_actor_idle_loop.rs`,
`framework_magic_contract/c5_c8_c13.rs` — call `apply(&RegistryWriteToken::for_test(), …)`.
`for_test()` is gated `#[cfg(any(test, feature = "test-registry-seam"))]`; `nmp-testing`
enables the feature on its `nmp-core` dependency. This keeps production sealed while tests
keep a registry-level entry point (they cannot use the `Kernel`-level front-door because
they hold no `Kernel`).

**Defense in depth (optional, recommended):** add a doctrine-lint rule
(`crates/nmp-testing/bin/doctrine-lint/rules/`) banning `registry_mut().apply(` outside
`crates/nmp-core/src/kernel/cache_serve/` and the `for_test()` seam, mirroring the
existing rule machinery (the crate already ships `rules/`, `fixtures/`, `tests.rs`).
Compile-time sealing is primary; the lint catches future `for_test()` misuse in prod.

---

## 5 — Consequences to verify

**(a) Profile claims fixed for free.** `register_profile_claim_interest` (`profile.rs:274`)
routes through `register_interest(.., Replace, "profile-claim")`, so the kind:0 interest
now store-serves on cold start: shape `{authors:[P], kinds:[0]}` → `AuthorKind` store query
→ `IngestParser` dispatch (`Kind0Parser`) → `ProfileCache.upsert_view` — exactly the path
`kind0-and-notice-investigation` A.1 identified as missing. The store-serve runs even on the
warm-reclaim branch the moment the interest is registered (the gap at
`kind0-and-notice-investigation:152-155`). **Liveness upgrade:** CacheOk→Live changes only
`lifecycle` ⇒ `plan_relevant_change == true` ⇒ recompile (wire upgrades OneShot→Tailing),
but shape is unchanged ⇒ same `completion_key` ⇒ the serve no-ops in
`served_interest_shapes` (`cache_serve/mod.rs:281-283`) or the pending-dedup
(`:284-288`). Correct.

**(b) Scattered serve/registration calls DELETED.** `startup.rs:199` and `startup.rs:255`
(`enqueue_interest_cache_serve`) deleted (absorbed by `register_interest`). `profile.rs:274`
+ `:277` and `claim_expansion_helpers.rs:169` + `:173` deleted (absorbed). `contacts.rs`'s
`push` loop (`:104-119`) and the manual `_deferred`+drain block (`:161-174`) collapse to one
`register_interests_batch`. `push_interest_and_serve` and `ensure_interest_and_serve`
(2 methods, ~30 lines) deleted. `enqueue_interest_cache_serve` stays (one internal caller:
`register_interest`); `enqueue_interest_cache_serve_deferred` stays (batch).

**(c) Net LOC.** ADD: `InterestWrite` (~10), `InterestRegistration` (~6),
`register_interest` (~22), `register_interests_batch` (~22), `RegistryWriteToken` (~12),
`apply` (~24, but it ABSORBS the deleted `ensure_sub`+`set_sub`+`push`+`push_if_changed`
bodies ~55 lines), `plan_relevant_change` (~8), `from_legacy_interest`/`drop_slot_by_key`
(~20), `OneshotApi::prepare` refactor (≈ net 0, splits existing `request`). DELETE:
`push` (~5), `push_if_changed` (~18), `legacy_identity`+`legacy_scope`+`legacy_key`
framing (~22), `withdraw` (~7), `push_interest_and_serve` (~14), `ensure_interest_and_serve`
(~20), `ensure_sub`/`set_sub` standalone (~40, folded into `apply`), `startup.rs` 2× serve
+ 2× set_sub (~6), `profile.rs` 2 lines, `claim_expansion_helpers.rs` 2 lines, `contacts.rs`
batch boilerplate (~25). **Three front-doors + four mutators → one front-door + one batch +
one sealed `apply`.** Net is negative (the absorbed bodies and the collapsed
contacts/startup/profile boilerplate exceed the added scaffolding). The dominant saving is
structural: 5 install patterns (store-first-layering §5 table) → 2 (single + batch).

---

## 6 — Safety analysis

| Invariant | Where | Preserved by |
|---|---|---|
| **D1 synchronous-drain timing** (first snapshot after install carries store data) | `cache_serve/mod.rs:132-160`; `ClaimProfile` → `maybe_emit_after_dispatch` `dispatch.rs:461` | `register_interest` calls `enqueue_interest_cache_serve` (= deferred + `run_cache_serve_step`) synchronously, same as today's front-doors. The post-`ClaimProfile` `maybe_emit_after_dispatch` still sees served events. |
| **Completion-key idempotency** (same-shape re-registration / liveness upgrade no-ops the serve) | `cache_serve/queries.rs:186`; `cache_serve/mod.rs:281-288` | `completion_key_for_interest` excludes lifecycle/hints/since/until/limit, so a `Replace` that changes only lifecycle (profile) or hints (claim-expansion) yields the same key ⇒ `enqueue_cache_serve` no-ops. `register_interest` still calls the serve (because `changed==true` for the trigger), but the serve self-dedups. |
| **Heartbeat anti-recompile** (`push_if_changed` optimization) | `cache_serve/mod.rs:182-188` | `plan_relevant_change` returns false for the deterministic, unchanged follow-feed interest ⇒ no trigger, no serve. |
| **Account-switch clear ordering** (`clear_served_interest_shapes` BEFORE new-identity serves) | `reconcile_follow_feed_after_identity_change` `contacts.rs:326`; `clear_served_interest_shapes` `cache_serve/mod.rs:382` | Unchanged — `clear_served_interest_shapes()` still runs first (`contacts.rs:326`), THEN `register_follow_feed_for_active_account` → `register_interests_batch`. Bootstrap `set_sub`-replaces-author still expressed as `Replace` (shape changes ⇒ `changed` ⇒ fresh serve after the clear). |
| **Multi-owner GC** (slot drops only when last owner leaves) | `registry.rs:103-115` | `drop_owner` untouched; `EnsureAbsent` attaches without clobbering (`registry.rs:59-79` body folded into `apply`). |
| **Store-first additive — REQ still fires when the store has data** (ADR-0045 R3) | bootstrap (`startup.rs`), discovery oneshots (`oneshot.rs`), profile claims | `register_interest` ALWAYS enqueues `InvalidateCompile` when `changed` (REQ half) in addition to the serve (store half). Discovery/bootstrap interests carry `is_indexer_discovery:true` (`startup.rs:193,248`, `oneshot.rs:136`, `profile.rs:268`) and the planner's bootstrap-indexer fallback still routes the REQ — the serve never replaces it. Flagged: any future `EnsureAbsent` caller whose re-registration is a no-op (`changed==false`) does NOT re-fire the REQ — correct, because the existing slot's REQ is already live; the recompile guard at `requires_recompile` (`trigger.rs:133`) and the registry's existing interest carry it. |
| **D6 no-panic / poisoned-lock degrade** | `cache_serve/mod.rs:312-318` | Unchanged — `register_interest` adds no locks; the ingest-dispatcher read is inside `enqueue_cache_serve` as before. |

**Skeptical notes / residual risks**

1. **OneshotApi refactor (caller #11)** is the only structural change beyond mechanical
   re-routing: it moves the `ensure_sub` call from `OneshotApi` into the Kernel. Risk: the
   borrow split (`prepare` returns owned data; Kernel then borrows `registry_mut()`) must
   not deadlock the existing `let registry = self.lifecycle.registry_mut()` borrow in
   `discovery.rs:227-229`. The refactor removes that inner borrow (registration moves to the
   front-door), so the borrow shape simplifies. Verify `requests/mod.rs:394` and
   `requests/event.rs:274` callers similarly.
2. **WithdrawInterest scope** (caller #12): confirm all external `*_interest_id()` producers
   are `Global`-scoped, else add a scope to the command or use `drop_slot_by_key` (any
   scope). Listed producers (`nmp-defaults`, `nmp-nip29`, `nmp-wot`) appear `Global`; verify.
3. **`FollowListChanged` payload consumers**: confirm nothing outside the compiler reads
   `new_follows` before deleting the redundant trigger (§4). If unsure, keep it — it
   coalesces harmlessly.
4. **`drop_owner` left un-sealed**: the mandate scopes the seal to *registration*. If a
   later mandate wants un-register sealed too, token-gate `drop_owner`/`drop_slot_by_key`
   the same way (trivial extension).

---

## Summary

- **One front-door:** `Kernel::register_interest(identity, interest, InterestWrite, reason)
  -> InterestRegistration` + `register_interests_batch(...)` for the follow-feed, both in
  `crate::kernel::cache_serve`. Store-serve (synchronous D1 drain) + recompile trigger are
  intrinsic and gated on a `changed` (newly-installed-or-plan-differs) predicate.
- **Policy enum:** `InterestWrite::{EnsureAbsent, Replace}` — preserves `ensure_sub` vs
  `set_sub`/`push` semantics.
- **Seal:** `RegistryWriteToken` (private field, minted only in `cache_serve`); the four
  mutators fold into one token-gated `InterestRegistry::apply`; `for_test()` seam for
  registry-only tests; optional doctrine-lint backstop.
- **Legacy surface DELETED:** `push`, `push_if_changed`, `legacy_identity`, `withdraw`(id
  framing), `push_interest_and_serve`, `ensure_interest_and_serve` — every caller (table in
  [`unified-interest-registration-callers.md`](unified-interest-registration-callers.md))
  migrated; the `InterestId`→`(scope,key)` bridge relocated as `SubIdentity::from_legacy_interest`.
- **Profile kind:0 bug fixed for free** (consequence §5a); net LOC negative (§5c); D1 /
  idempotency / account-switch-clear / multi-owner-GC / store-first-additive all preserved
  (§6). Only the OneshotApi path needs a (designed) construction/registration split.
