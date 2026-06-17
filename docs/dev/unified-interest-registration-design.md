# Unified Interest Registration — Design

**Status:** Design — **codex-validated 2026-06-17; sound to implement** with the four
amendments in "§0 Codex amendments (binding)" below, which OVERRIDE the body where they
conflict. Date: 2026-06-17.
**Mandate:** ONE front-door for "register an interest in events for X", store-first
by construction (serve-from-store + relay REQ are both intrinsic to it). The legacy
`push` / `push_if_changed` (registry) and `push_interest_and_serve` (kernel) surface
is **completely deleted**, every caller migrated. No parallel registration path remains.

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

## 0 — Current surface (what exists today)

### Registry write methods (`crates/nmp-core/src/subs/registry.rs`)

| Method | Line | Vis | Semantics | Returns |
|---|---|---|---|---|
| `ensure_sub` | `68` | `pub` | register-if-absent; never clobbers an existing `(scope,key)` slot | `bool` newly-installed |
| `set_sub` | `86` | `pub` | upsert: attach owner + **replace** the slot's interest unconditionally | `()` |
| `drop_owner` | `103` | `pub` | detach one owner; drop the slot when the last owner leaves | `bool` slot-removed |
| `push` | `135` | `pub` | LEGACY `InterestId` surface → `set_sub(legacy_identity(interest), interest)` | `()` |
| `push_if_changed` | `149` | `pub(crate)` | LEGACY: `set_sub` only when stored `shape != incoming.shape` | `bool` absent-or-changed |
| `withdraw` | `163` | `pub` | LEGACY: remove every slot whose `key == legacy_key(id)` | `()` |
| `legacy_key` | `221` | `pub(crate)` | `InterestId` → `SubKey` bridge | `SubKey` |
| `legacy_scope` | `225` | private | `InterestScope` → `SubScope` | `SubScope` |
| `legacy_identity` | `236` | private | `LogicalInterest` → `SubIdentity` (owner = `"legacy-single-owner"`) | `SubIdentity` |

The registry is owned by `SubscriptionLifecycle`; mutable access is `registry_mut()`
(`crates/nmp-core/src/subs/lifecycle.rs:378`, `pub`). The registry has **no** access to
the Kernel's `pending_cache_serves` / `served_interest_shapes` / store, so it cannot
serve — serving is a Kernel concern (store-first-layering-investigation §3, Chokepoint A).

### Kernel install front-doors (`crates/nmp-core/src/kernel/cache_serve/mod.rs`)

| Method | Line | Vis | Body |
|---|---|---|---|
| `push_interest_and_serve` | `197` | `pub(crate)` | `push_if_changed` → on change: `InvalidateCompile` + `enqueue_interest_cache_serve` |
| `ensure_interest_and_serve` | `225` | `pub(crate)` | `ensure_sub` → on newly-installed: `InvalidateCompile` + `enqueue_interest_cache_serve` |
| `enqueue_interest_cache_serve` | `154` | `pub(crate)` | `enqueue_interest_cache_serve_deferred` + `run_cache_serve_step` (the D1 synchronous drain) |
| `enqueue_interest_cache_serve_deferred` | `170` | `pub(in crate::kernel)` | derive completion key + `enqueue_cache_serve` (enqueue only) |
| `run_cache_serve_step` | `347` | `pub(crate)` | drain one aggregate-budget chunk |
| `clear_served_interest_shapes` | `382` | `pub(in crate::kernel)` | clear completion set + pending queue (account switch) |

The two front-doors centralise `ensure_sub` and `push_if_changed`, but **`set_sub` has
no front-door** — `startup.rs` and `profile.rs` call `set_sub` directly and (must
remember to) call `enqueue_interest_cache_serve` separately. That per-caller pattern is
the bug (`store-first-layering-investigation` §5; the profile path forgets entirely —
`kind0-and-notice-investigation` A.1).

`completion_key_for_interest(sub_key, shape)` (`cache_serve/queries.rs:186`) hashes
`(sub_key, authors, kinds, tags, addresses)` — **lifecycle / since / until / limit /
hints are NOT in the key**. This is the idempotency anchor the whole design leans on.

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
`changed` predicate is a plan-relevant diff (see §3) that subsumes both.

### `register_interests_batch` (the follow-feed mode, §4)

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

## 3 — Complete elimination of `push` / `push_if_changed` / `push_interest_and_serve`

### The `changed` predicate (replaces `push_if_changed`'s shape-only check)

```rust
/// True iff the stored interest differs from the incoming one in a field the
/// planner compiles on. Subsumes `push_if_changed`'s shape-only check and adds
/// lifecycle (OneShot↔Tailing wire upgrade) and hints (claim-expansion W7 REQs).
/// `since`/`until`/`limit` are excluded (watermark+relay refinement owns them,
/// matching `completion_key_for_interest`'s exclusions, queries.rs:183).
fn plan_relevant_change(stored: &LogicalInterest, incoming: &LogicalInterest) -> bool {
    stored.shape != incoming.shape
        || stored.lifecycle != incoming.lifecycle
        || stored.hints != incoming.hints
        || stored.is_indexer_discovery != incoming.is_indexer_discovery
}
```

**No follow-feed-heartbeat regression:** `follow_feed_interest(pubkey, kinds)`
(`contacts.rs:52`) is deterministic — same shape, `Tailing`, empty hints,
`is_indexer_discovery:false` every call — so a re-sync with an unchanged follow set yields
`changed == false` for every author ⇒ no trigger, no serve (the exact
`push_if_changed` optimization, `cache_serve/mod.rs:182-188`, preserved).

### Complete caller table

Legend: **C** = `crate::subs::CompileTrigger`. Every row's end state has zero references
to `push`/`push_if_changed`/`push_interest_and_serve`/`set_sub`/`ensure_sub` outside the
sealed `apply`.

| # | Caller (`file:line`) | Today | Migration |
|---|---|---|---|
| 1 | **`ActorCommand::PushInterest` arm** `actor/dispatch.rs:1326` → `push_interest_and_serve` | `push_if_changed` (legacy id→synthetic owner) | Derive identity from the interest (see "legacy-id bridge" below) and call `register_interest(id, interest, InterestWrite::Replace, "push-interest")`. |
| 2 | `push_interest_and_serve` `cache_serve/mod.rs:197` | calls `push_if_changed` | **DELETE** the method. |
| 3 | `push_if_changed` `registry.rs:149` | only caller is #2 | **DELETE**. |
| 4 | `InterestRegistry::push` `registry.rs:135` | callers #6 (prod) + tests | **DELETE**; `legacy_identity` (`:236`) deleted with it. |
| 5 | **`ensure_interest_and_serve`** `cache_serve/mod.rs:225` (callers: `dispatch.rs:1346` EnsureInterest, `kernel_action.rs:115` open_uri, `mod.rs:2503` open_interest_sub) | `ensure_sub` front-door | **DELETE** the method; each caller calls `register_interest(.., InterestWrite::EnsureAbsent, reason)`. `open_interest_sub` (`mod.rs:2494`) becomes a 1-line delegate (kept — used by `feed_verbs.rs:35` reducer + has the `close_interest_sub` counterpart). |
| 6 | **`sync_follow_feed_interests`** `ingest/contacts.rs:108,117` (`push`) + `:171,173` (`_deferred`×N + drain) | legacy `push` + manual batch serve | Build `(identity, interest)` per follow + self (identity = follow-feed owner + follow-feed key, §"follow-feed identity"), call `register_interests_batch(items, InterestWrite::Replace, "follow-list-changed")`. Deletes the separate `push` loop AND the manual `_deferred`+`run_cache_serve_step` block (lines `104-119` and `161-174` collapse into one batch call). |
| 7 | **`register_oneshot_discovery_interest`** `startup.rs:172` (`set_sub:195` + `enqueue_interest_cache_serve:199`) | manual set_sub + manual serve | `register_interest(identity, interest, InterestWrite::Replace, "bootstrap-oneshot-discovery")`. Deletes the explicit `enqueue_interest_cache_serve` call (`:199`). |
| 8 | **`register_tailing_self_kinds_interest`** `startup.rs:216` (`set_sub:250` + `enqueue_interest_cache_serve:255`) | manual set_sub + manual serve | `register_interest(identity, interest, InterestWrite::Replace, "bootstrap-self-kinds")`. Deletes the explicit serve (`:255`). |
| 9 | **`register_profile_claim_interest`** `profile.rs:274` (`set_sub`) + `:277` (`ViewOpened` trigger), **NO serve** (THE BUG) | manual set_sub + trigger, no serve | `register_interest(identity, interest, InterestWrite::Replace, "profile-claim")`. Deletes the bare `set_sub` + `enqueue_trigger(ViewOpened)` (3 lines → 1). **Fixes the kind:0 store-first bug for free** (consequence §5a). |
| 10 | **`advance_to_phase2`** `claim_expansion_helpers.rs:169` (`set_sub`) + `:173` (`ViewOpened`) | manual set_sub (hint update) + trigger | `register_interest(identity, interest, InterestWrite::Replace, "claim-expansion-phase2")`. Hints differ ⇒ `changed==true` ⇒ recompile fires (emits W7 hint REQs); shape unchanged ⇒ completion key unchanged ⇒ serve is an idempotent no-op (safe; §5 safety). Deletes the bare `set_sub` + trigger. |
| 11 | **`OneshotApi::request`** `subs/oneshot.rs:142` (`ensure_sub` on a borrowed `&mut InterestRegistry`) | direct `ensure_sub`, NO serve | **Resists migration** — see "the OneshotApi bridge" below. |
| 12 | `ActorCommand::WithdrawInterest` arm `dispatch.rs:1330` (`withdraw`) + `sync_follow_feed_interests` `contacts.rs:88` (`withdraw`) | legacy `withdraw(id)` | Un-register, not register — see "withdraw" below. Migrated off the `InterestId` surface to `drop_owner`. |

External crates that **send** `ActorCommand::PushInterest`/`WithdrawInterest` (they never
touch the registry directly — they go through arm #1/#12, so migrating the arm covers them
transparently, NO external API change): `nmp-wot/src/runtime.rs:200,204`;
`nmp-ffi/src/lib.rs:1923`; `nmp-defaults/src/runtimes.rs:343,346,432,437,442,447`
(giftwrap-inbox + zap-receipts controllers); `nmp-nip29/src/action/discover.rs:65`.

### The legacy-id → `(scope, key)` bridge (caller #1, and #12)

`ActorCommand::PushInterest(interest)` carries a `LogicalInterest` with an `InterestId`
and no `SubIdentity`. Today `push` maps it via `legacy_identity`
(`registry.rs:236`): owner = `SubOwnerKey::new("legacy-single-owner")`,
key = `legacy_key(id)` = `SubKey::builder("legacy-interest-id").with(id.0).finish()`,
scope = `legacy_scope` (`Account(pk)` or `Global`). **Keep this exact derivation** but
relocate it as a pure constructor on `SubIdentity` (or a free fn) used by the dispatch
arm — it is no longer a registry *mutator*, just an id→identity mapping:

```rust
// e.g. crate::subs::sub_key
impl SubIdentity {
    /// Map a legacy `InterestId`-keyed `LogicalInterest` onto the (owner,key,scope)
    /// triple, preserving the single shared synthetic owner so withdraw-by-id keeps
    /// nuking the slot regardless of owners (the legacy semantics, see below).
    pub(crate) fn from_legacy_interest(interest: &LogicalInterest) -> Self { ... }
}
```

**Single-synthetic-owner semantics — does anything depend on them the modern model
can't express?** The legacy model uses ONE shared owner `"legacy-single-owner"` across
ALL pushed interests, but distinct `legacy_key(id)` per id ⇒ each interest is its own
`(scope, key)` slot with exactly one owner. Consequences that must be preserved:

1. **Two controllers pushing the SAME id** → same key, same single owner → the second
   `Replace` overwrites (today: `set_sub`). Preserved by `Replace` + shared owner.
2. **`withdraw(id)` removes the whole slot** regardless of owner count (today:
   `slots.retain(key != legacy_key(id))`, `registry.rs:168`). Because the slot has exactly
   one owner (the shared synthetic one), `drop_owner(from_legacy_interest)` removes the last
   owner ⇒ slot dropped — **identical** outcome. So `withdraw(id)` migrates to
   `drop_owner(SubIdentity::from_legacy_interest(&interest_for(id)))`. The WithdrawInterest
   command carries only the `InterestId`, not the full interest — but `drop_owner` keys on
   `(scope, key, owner)` and scope is needed. Mitigation: `WithdrawInterest` currently
   relies on `withdraw` scanning every scope (`registry.rs:165-168`); reconstruct the
   identity with the same synthetic owner and BOTH scopes is unnecessary because all legacy
   external pushers (`nmp-defaults`, `nmp-nip29`, `nmp-wot`) use `Global` scope (their
   `LogicalInterest`s are `InterestScope::Global`/`ActiveAccount` → `SubScope::Global` via
   `legacy_scope`). **Verify** at implementation: grep the external `*_interest_id()`
   constructors; if any are `Account`-scoped, `WithdrawInterest` must carry the scope (a
   small command-field addition) OR keep a `drop_owner_by_key(key)` registry helper that
   removes the slot under any scope (the `withdraw` body, minus the `InterestId` framing).
   Recommended: add `InterestRegistry::drop_slot_by_key(&RegistryWriteToken, key)` (reads
   like today's `withdraw` retain, but token-sealed and keyed by `SubKey` not `InterestId`),
   so the legacy `withdraw` + `legacy_key` framing is fully deleted.

This bridge means **nothing depends on single-synthetic-owner semantics that the modern
`(scope,key)`+owner model can't express** — the legacy surface was always a single-owner
projection of the modern model (the registry doc says so verbatim, `registry.rs:25-28`).

### The OneshotApi bridge (caller #11 — the only one that resists)

`OneshotApi::request` (`subs/oneshot.rs:105-152`) is invoked with a borrowed
`&mut InterestRegistry` (`discovery.rs:227-229,268-270`; also `requests/mod.rs:394`,
`requests/event.rs:274`) — it does NOT hold a `Kernel`, so it cannot call the
`Kernel`-level front-door or `enqueue_interest_cache_serve`. It calls `ensure_sub`
directly (`:142`) with NO store-serve today. Two problems: (a) it bypasses the front-door;
(b) it never store-serves (a latent gap — discovery oneshots fetch UNKNOWN ids/pubkeys, so
the store is usually empty, but ADR-0045 R3 says store-first is additive: serve AND REQ,
never serve-instead-of-REQ — and the REQ is non-negotiable here, §"store-first additive").

**Bridge:** split `OneshotApi::request` into pure-construction + Kernel-driven
registration. `OneshotApi` keeps ONLY its token bookkeeping; it returns the
`(SubIdentity, LogicalInterest, OneshotToken, InterestId)` to the Kernel, which registers
via the unified front-door:

```rust
// OneshotApi: pure — mints token, derives identity+interest, records bookkeeping.
//   NO registry mutation. Returns the identity+interest for the Kernel to register.
pub fn prepare(&mut self, scope, shape, hints) -> (OneshotToken, InterestId, SubIdentity, LogicalInterest)

// Kernel (discovery.rs): the actual registration goes through the front-door.
let (token, iid, identity, interest) = self.oneshot.prepare(scope, shape, hints);
self.register_interest(identity, interest, InterestWrite::EnsureAbsent, "oneshot-discovery");
self.pending_discovery_oneshots.insert(iid, token);
```

`release` still calls `OneshotApi::release` → `drop_owner` (un-register, out of scope of
the registration seal; `drop_owner` stays callable, or also token-gate it for symmetry —
the design keeps `drop_owner` un-sealed since withdraw is not "registering an interest").
This both routes oneshot registration through the single front-door AND gives discovery
oneshots the additive store-serve for free (a strict improvement; the serve no-ops when the
id/pubkey is genuinely unknown — `enqueue_cache_serve` marks uncovered/empty shapes served,
`cache_serve/mod.rs:292-297`). The wire REQ still fires (EnsureAbsent → `changed` →
`InvalidateCompile`), so store-first stays additive.

End state: `rg "push_if_changed|\.push\(|push_interest_and_serve"` over
`crates/*/src` (excluding `Vec::push`/`VecDeque::push` and test fixtures) returns **zero**
registry-registration hits.

---

## 4 — The follow-feed batch (caller #6, detail)

`sync_follow_feed_interests` (`ingest/contacts.rs:84-175`) today:
1. `withdraw` stale ids (`:86-90`),
2. `push` one interest per follow + self (`:105-119`),
3. rebuild `timeline_authors` (`:125-129`),
4. `_deferred` serve per author + ONE `run_cache_serve_step` (`:161-174`).

After migration:
1. **Un-register** the prior follow set via `drop_slot_by_key` (or `drop_owner` per
   reconstructed identity) — `follow_feed_interest_ids` (`mod.rs:870`,
   `BTreeSet<InterestId>`) becomes `BTreeSet<SubKey>` (or keep ids + recompute keys).
2. Build `Vec<(SubIdentity, LogicalInterest)>` for follows + self.
3. `register_interests_batch(items, InterestWrite::Replace, "follow-list-changed")` —
   this does the registry writes (steps-2-and-4 merged), one coalesced trigger, ONE drain.
4. `timeline_authors` rebuild (`:125-129`) stays — it is a derived read-cache, not a
   registration.

**Follow-feed identity.** Each follow maps to: owner = `SubOwnerKey::new("kernel:follow-feed")`
(single stable owner mirroring the bootstrap owner `startup.rs:82`), key =
`SubKey::builder("follow-feed").with(pubkey).with(kinds_sorted).finish()` (replacing
`legacy_key(contact_list_authors_interest_id(...))`), scope = `Global`
(`InterestScope::Global`, `contacts.rs:57`). Distinct pubkey/kinds ⇒ distinct slot, single
owner ⇒ `drop_*` GCs it on re-sync — identical to today's `withdraw`+`push`.

It is the SAME mechanism as `register_interest`, batched (one drain, not N) — satisfying
ADR-0045 §5 (`cache_serve/mod.rs:16-32`) without a second path. The `FollowListChanged`
trigger the callers already enqueue (`on_active_contacts_changed` `contacts.rs:219`,
`register_follow_feed_for_active_account` `:293`, `reconcile…` `:332`) is **redundant for
recompile** now (the batch enqueues `InvalidateCompile`, and the compiler walks the full
registry ignoring the trigger payload — `trigger.rs:115` "the compiler itself does not
inspect this field"). Keep `FollowListChanged` only if a non-compiler consumer reads
`new_follows` (verify `nmp-defaults`/publish consumers, `publish.rs:715`); otherwise it can
be dropped. Both triggers coalesce in one tick (`lifecycle.rs:396` "coalesced with siblings
until the next drain_tick") ⇒ one recompile either way — no double compile.

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
  framing), `push_interest_and_serve`, `ensure_interest_and_serve` — every caller (table §3)
  migrated; the `InterestId`→`(scope,key)` bridge relocated as `SubIdentity::from_legacy_interest`.
- **Profile kind:0 bug fixed for free** (consequence §5a); net LOC negative (§5c); D1 /
  idempotency / account-switch-clear / multi-owner-GC / store-first-additive all preserved
  (§6). Only the OneshotApi path needs a (designed) construction/registration split.
