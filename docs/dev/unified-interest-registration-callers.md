# Unified Interest Registration — Current Surface & Caller Migration

Companion to [`unified-interest-registration-design.md`](unified-interest-registration-design.md),
which contains the §0 codex amendments, front-door API design, sealing, safety analysis,
and summary. This file covers the detailed current-surface enumeration (§0) and the
complete caller-migration table (§3) and follow-feed batch detail (§4).

> Status note, 2026-06-26: the follow-feed batch material in this investigation
> is historical after #2092 M5. Active-user follows no longer use
> `sync_follow_feed_interests`; they are expressed as a ReducedSource that
> recompiles dependent interests.

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
