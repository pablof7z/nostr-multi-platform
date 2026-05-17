# Subscription Compilation §3 — The Compilation Pipeline

> Parent: `docs/design/subscription-compilation.md`.
> Read first: [intro.md](intro.md) for the `LogicalInterest` shape this stage consumes.

The compiler is a pure function plus a small amount of state (the mailbox cache and the active plan registry). It runs whenever a recompilation trigger fires (§4) and produces a `CompiledPlan` that the wire-emitter applies as a diff against the relay sockets.

## 3.0 Pipeline overview

```
   logical_interests:                 mailbox_cache + relay_config
   Vec<LogicalInterest>                       │
            │                                 │
            ▼                                 ▼
   ┌───────────────────────────────────────────────────┐
   │ Stage 1: Resolve authors → mailboxes              │  (§3.1)
   │   each author → { write, read, both, missing }    │
   └───────────────────────────────────────────────────┘
            │
            ▼
   ┌───────────────────────────────────────────────────┐
   │ Stage 2: Indexer fallback for missing mailboxes   │  (§3.2)
   │   missing → enqueue kind:10002 probe              │
   │   missing-author reads → indexer set (read only)  │
   └───────────────────────────────────────────────────┘
            │
            ▼
   ┌───────────────────────────────────────────────────┐
   │ Stage 3: Per-relay shape merge                    │  (§3.3)
   │   group interests by target relay URL             │
   │   merge compatible shapes inside each relay       │
   │   refuse merges that would change semantics       │
   └───────────────────────────────────────────────────┘
            │
            ▼
   ┌───────────────────────────────────────────────────┐
   │ Stage 4: Plan-id binding                          │  (§3.4)
   │   compute plan_id = hash(interest_set,            │
   │                          mailbox_snapshot,        │
   │                          merge_lattice_version)   │
   │   stable across no-op recompilations              │
   └───────────────────────────────────────────────────┘
            │
            ▼
   CompiledPlan { plan_id, per_relay: Vec<RelayPlan> }
```

The wire-emitter (`crates/nmp-core/src/kernel/wire.rs`, to be added) diffs the new plan against the current wire-sub registry: opens new REQs, closes orphaned ones, leaves stable assignments untouched.

## 3.1 Stage 1 — Resolve authors to mailboxes

Inputs: every `LogicalInterest` with non-empty `shape.authors` or non-empty `shape.tags[#p]`; the mailbox cache populated by `ingest_relay_list` (`crates/nmp-core/src/kernel/ingest.rs:209-233`).

Output: an `AuthorRouting` per author per direction:

```rust
pub struct AuthorRouting {
    pub author: Pubkey,
    pub direction: RoutingDirection,        // Outbox or Inbox
    pub relays: BTreeSet<RelayUrl>,         // resolved write/read/both
    pub source: RoutingSource,              // Nip65 | UserConfigured | Hint
    // Note: there is no RoutingSource::Indexer variant — indexer fallback is
    // modeled as UserConfigured { category: Indexer } (see diagnostics.md §5.0).
    // This keeps the four-lane discipline strict: indexers are a sub-category of
    // user-configured, not a fifth lane.
    pub freshness_ms: Option<u64>,          // age of the kind:10002 record
}

pub enum RoutingDirection {
    Outbox,    // for `authors:` filters — the author's *write* relays
    Inbox,     // for `#p:` filters    — the tagged author's *read* relays
}
```

Direction is decided by the interest's filter shape per `docs/product-spec/subsystems.md` §7.3:

| Interest shape | Direction | Source per author |
|---|---|---|
| Non-empty `authors`, no `#p` | Outbox | author's `write_relays ∪ both_relays` |
| Empty `authors`, non-empty `#p` | Inbox | tagged author's `read_relays ∪ both_relays` |
| Both populated | Outbox primarily; Inbox interests split (see §3.3) | both |
| Neither populated | (handled by stage 3 as "use active-account read relays") | — |

`docs/product-spec/subsystems.md` §7.3 specifies one explicit override: DMs (NIP-17 gift-wraps, M9) fail closed if recipient inbox relays are missing. The compiler enforces this by refusing to produce a plan for an interest tagged `privacy = FailClosed` if any tagged-pubkey inbox lookup has empty relays or was sourced from `UserConfigured { category: Indexer }` (meaning no NIP-65-declared inbox exists). §7 details the publish-side enforcement.

## 3.2 Stage 2 — Indexer fallback for unknown mailboxes

The indexer set is a kernel-configured `Vec<RelayUrl>` (default: a small curated list; user-configurable in `AppConfig`). Today's `crates/nmp-core/src/relay.rs:2` is the placeholder for one indexer relay (`purplepag.es`); the v1 indexer set lives in `AppConfig.indexer_relays`.

Two distinct behaviours:

1. **Mailbox probe.** For every author with `mailbox_cache.get(author) == None`, the compiler emits a `IndexerProbe { author }` side effect on the plan. The probe registers as its own short-lived `LogicalInterest { shape: { kinds: [10002], authors: [author], limit: 1 }, lifecycle: OneShot, scope: Global }`. Recompilation triggers (§4 trigger A1) re-route the original interest once the kind:10002 lands.
2. **Read fallback.** For a `RoutingDirection::Outbox` interest whose author has no known mailboxes, the compiler routes the interest to the indexer set **as read-only fallback**. Per `docs/product-spec/subsystems.md` §7.3 line 99: "fall back to indexer set for reads only; do not publish to indexers." The resulting `AuthorRouting` carries `source: RoutingSource::UserConfigured` with a `UserConfiguredRelayFact { category: UserConfiguredCategory::Indexer }` record flowing to the diagnostic surface. The four-lane view (§5) renders "author X is being served by indexer Y because we have no mailbox for them" by examining the `category` subcategory — the Indexer is a sub-category of lane 4 (User-configured), not a fifth lane.

Bounded: a single author's indexer probe is enqueued at most once per `compiler_probe_window_secs` (default 60s) to prevent thundering-herd probes if a screen of N rows all claim the same unknown pubkey.

## 3.3 Stage 3 — Per-relay shape merge

After Stage 1, every interest has one or more `(relay_url, sub_shape)` assignments. Stage 3 groups by relay URL and merges shapes where merging preserves semantics.

### Merge rules (the lattice)

Two `InterestShape`s `A` and `B` are **mergeable on relay R** iff:

1. `A.kinds == B.kinds` **or** one is empty (wildcard absorbs).
2. `A.tags.keys() == B.tags.keys()` (same tag dimensions) **and** the union of values per dimension stays under the relay's per-filter limit (default 1000).
3. `A.since` and `B.since`: merged `since = min(A.since, B.since)` *only if* both are present or both absent. Mixing a bounded interest with an unbounded one is **not** merged (would broaden the bounded one's window).
4. `A.until` and `B.until`: same rule, mirror of (3) with `max`.
5. `A.limit` and `B.limit`: mergeable iff both are absent. If either has a `limit`, **do not merge** — broadening would mask the limit's intent.
6. `A.lifecycle == B.lifecycle`. Tailing and one-shot do not merge (one-shot would never close).
7. `A.event_ids` and `B.event_ids`: merge by union, capped at the relay's per-filter `ids` limit.

When mergeable, the merged shape is `{ authors: A.authors ∪ B.authors, ... }`. The merged interest tracks both originating `InterestId`s so per-event dispatch back to consumers stays correct.

When not mergeable, the two interests get distinct sub-shapes on the same relay, producing two distinct REQs. That is fine and expected.

Open question 2 in the parent index (`subscription-compilation.md`) covers the `limit`-only corner case formally.

The NMP merge lattice is simpler than Applesauce's `selectOptimalRelays` greedy set-cover
(`docs/research/applesauce/outbox.md` §3, `relay-selection.ts:14-93`): Applesauce optimizes
the number of relay connections by picking a minimum covering set across all contacts. NMP's
Stage 3 merges shapes per relay but does not eliminate relays — the set-cover optimization
(capping to `maxConnections`) is a future extension. For M2, every declared write relay gets
a REQ; relay-count optimization is open question 8 (future ADR).

### Per-relay output

```rust
pub struct RelayPlan {
    pub relay_url: RelayUrl,
    pub role_tags: BTreeSet<RoutingSource>,   // why this relay is in the plan
    pub sub_shapes: Vec<SubShape>,            // each emits one REQ
}

pub struct SubShape {
    pub shape: InterestShape,                  // canonical, post-merge
    pub originating_interests: Vec<InterestId>,
    pub canonical_filter_hash: String,         // for ADR-0007 WireSubscriptionStatus
}
```

The wire-emitter renders each `SubShape` as exactly one `REQ` on `relay_url` with a sub-id of `c{plan_id}-r{relay_idx}-s{shape_idx}`. The sub-id is meaningful only to the kernel; diagnostics use `canonical_filter_hash` for stable identity across re-emission.

## 3.4 Stage 4 — Plan-id binding

`plan_id` is the **stable identity** the platform observes for diagnostic continuity. It answers: "did this recompilation actually change anything observable?"

Definition: **hash only mailboxes referenced by the current interest set**, not the whole
cache. This resolves open question 1 in the parent index — choosing this scope means that
an unrelated author's kind:10002 arriving does not churn plan-ids for unrelated interests.

```
// referenced_pubkeys = union of all shape.authors and shape.tags[#p] across interest_set
referenced_pubkeys = interest_set.iter()
    .flat_map(|i| i.shape.authors.iter().chain(i.shape.tags.get("#p").unwrap_or(&[])))
    .collect::<BTreeSet<Pubkey>>();

plan_id = blake3(
    sorted(interest_set.iter().map(|i| (i.id, i.shape, i.scope, i.lifecycle))),
    sorted(referenced_pubkeys.iter().filter_map(|pk| mailbox_cache.get(pk))
              .map(|ml| (ml.pubkey, ml.created_at, sorted(ml.write), sorted(ml.read)))),
    INDEXER_SET_VERSION,
    USER_CONFIGURED_RELAYS_VERSION,
    MERGE_LATTICE_VERSION,
)
```

Properties:

- **Recompilation with no change ⇒ same plan-id.** If `ingest_relay_list` (`crates/nmp-core/src/kernel/ingest.rs:218-221`) deduplicates and decides not to replace a stale mailbox, no plan-id churn.
- **Adding an interest changes plan-id even if no new wire REQ results.** Two interests can merge into the same SubShape; the plan-id changes because the *interest set* changed. The platform diagnostic correctly reports "logical-interest count went up; wire-sub count did not."
- **A new kind:10002 for a referenced author changes plan-id.** The hash only covers mailboxes for authors the current interest set touches, so a kind:10002 for an unrelated pubkey (not in any active interest's `authors` or `#p`) does not churn plan-ids.
- **A new kind:10002 for an unreferenced author does NOT change plan-id.** This is the key
  difference from hashing the whole snapshot. The wire-emitter's diff is unaffected. This
  property is critical for D8 (reactivity contract — §1.5): recompilation work must be bounded
  by what's open, not by the size of the total mailbox cache. Hashing only referenced mailboxes
  ensures recompile cost scales with `|interest_set| × |referenced_authors|`, not with the
  entire cache.
- **Indexer set change changes plan-id.** Operator config edits surface immediately.

The `plan_id` is stored on `CompiledPlan` and rendered into `LogicalInterestStatus` (extending the record at `crates/nmp-core/src/kernel/mod.rs:147-154` with `plan_id: String, plan_generation: u64`). Tests in §9 assert plan-id stability across no-op recompilations.

## 3.5 Migration of existing functions

This is the binding contract: each function in `crates/nmp-core/src/kernel/requests.rs` and `crates/nmp-core/src/kernel/ingest.rs` either disappears, becomes thin glue over the compiler, or graduates into a typed module. The compiler does not coexist with the old planner; M2 replaces it.

| Current function (file:line) | M2 replacement |
|---|---|
| `startup_requests` (requests.rs:50-106) | **Relocated outside `nmp-core` per D0.** The seed timeline / kind:0 / kind:10002 / kind:3 bootstrap is social graph knowledge — not kernel substrate. It moves to `nmp-nip01` / `nmp-nip02` / the demo app's startup sequence. `nmp-core` only executes interests registered by modules; it does not hard-code which interests those are. The TEST_PUBKEY-specific bootstrap (line 71-82) moves to the demo app (or a test fixture in `nmp-testing`). The kernel's `ActorStart` handler calls `compiler.recompile(Trigger::Startup)`, which is a no-op until a module registers interests. |
| `open_author` (requests.rs:118-140) | Registers three `LogicalInterest`s scoped `ActiveAccount` (kind:10002, kind:0, kinds:1+6 for author); calls `compiler.recompile(Trigger::ViewOpen)`. Refcount stays — but it lives on `InterestId` now, not on `ViewInterest { key, refcount }`. The `can_send` gate disappears: the compiler always produces a plan; the wire-emitter is the only thing that may queue deferrals. |
| `open_thread` (requests.rs:142-168) | Registers a `Thread { event_id }` view-module spec; the view module returns interests with `event_ids` and `#e`-tag shapes. Hydration cascade in `prepare_thread_requests` (requests.rs:441-466) becomes part of the view module's `reduce` returning new interests when new event ids surface. |
| `open_firehose_tag` (requests.rs:170-200) | Registers one `LogicalInterest { shape: { kinds: [1], tags: { #t: [tag] } }, scope: ActiveAccount, lifecycle: Tailing }`. Routes to active-account read relays per §3.1 table. |
| `claim_profile` / `release_profile` (requests.rs:202-263) | Registers/unregisters one `LogicalInterest { shape: { authors: [pk], kinds: [0], limit: 1 }, lifecycle: OneShot }` per claim. Refcount of distinct consumers becomes the `InterestId` claim set inside the registry. **Dedup of (pk, kinds=[0]) across N timeline rows yields one merged SubShape and one wire REQ** — this is what bug-extinction "1000 avatars do not produce 1000 REQs" verifies. The refcount must be an integer counter, not a boolean presence flag — Applesauce gotcha `75ef7d5f` (`docs/research/applesauce/gotchas.md` §G2) shows the boolean model causes leaks when two consumers share a claim and one unsubscribes. |
| `close_author` / `close_thread` (requests.rs:265-311) | Drop interests by `InterestId`; recompile with `Trigger::ViewClose`. Wire-emitter closes orphaned REQs. The "warm-close" grace from the view-warmth doctrine (`docs/design/kernel-substrate.md` §3 "lifecycle") is the compiler's, not the view's — interests stay registered for the warmth window after their last claim. |
| `close_subscriptions_with_prefixes` (requests.rs:313-331) | **Deleted.** The wire-emitter closes by `WireSubId`, which is the compiler's diff output. String-prefix matching of sub-ids is a 2026-05-period scaffold that the compiler removes. |
| `pending_view_requests` (requests.rs:333-355) | Becomes `compiler.flush_deferred_for_relay(role, url)`: called when a relay reconnects (§4 trigger A3). The compiler resubmits its current plan against that relay's slot. |
| `firehose_requests` (requests.rs:357-372) | Replaced as described above for `open_firehose_tag`. The `diag-firehose-N` sub-id scheme goes away — `canonical_filter_hash` plus `plan_id` give stable identity. |
| `pending_profile_claim_requests` (requests.rs:374-388) | Disappears. Claims are interests; the compiler is the only thing that decides "this interest needs a REQ." |
| `profile_claim_request` (requests.rs:390-402) | Disappears. The compiler routes claimed-profile interests through Stage 1; indexer fallback (Stage 2) handles the no-mailbox case. |
| `author_requests` (requests.rs:404-439) | Disappears (replaced by `open_author`'s interest registration). |
| `prepare_thread_requests` / `enqueue_thread_*` / `maybe_open_thread_hydration` (requests.rs:441-528) | Move to a `ThreadViewModule` in `nmp-nip10`. The hydration cascade is `view_module.reduce(...)` returning additional interests as new event ids surface in store. |
| `req` (requests.rs:530-556) | **Deleted.** Replaced by the wire-emitter's `emit_req(relay_url, sub_id, filter)`. No call site outside the wire-emitter is permitted to construct a REQ. |
| `defer_outbound` (requests.rs:558-568) | Moves to the wire-emitter; deferral is per-relay, keyed by URL, not by role. |
| `ingest_relay_list` (ingest.rs:209-233) | Stays, but emits a `Trigger::Nip65Arrived { pubkey }` event (§4 trigger A1) on a material update. Becomes the producer side of the recompilation cycle. |
| `ingest_profile` / `ingest_contacts` / `ingest_timeline_event` (ingest.rs:166-279) | Unchanged in storage shape. Their relevance to compilation is that they feed the view modules' projections (per `docs/design/reactivity/view-deltas-and-projections.md`). |
| `should_store_event` (ingest.rs:268-279) | Unchanged. Per-sub-id string filtering goes away when sub-ids become `c{plan}-r{relay}-s{shape}`, but the predicate switches to "is this event id covered by an active interest?" — a `compiler.is_covered(event)` call. |
| `maybe_open_timeline` (ingest.rs:329-365) | The "seed-contacts arrive → open union timeline" logic moves to `nmp-nip02` (follows/contacts module), not to an `nmp-core` helper. D0: `nmp-core` must not know the social graph. The contacts module watches the kind:3 projection and registers a `Timeline { authors: union }` interest on behalf of whichever view asked for it. |

What this migration does **not** do (deferred per parent index open questions 3, 6, 7):

- It does not move the action ledger into M2 — `SendNote` lands in M6.
- It does not implement LMDB persistence for the mailbox cache — M3.
- It does not implement NIP-77 watermarks — M4.
- It does not add a per-author indexer-fallback ledger row — open question 3.
- It does not move social bootstrap (follow-list timeline, account kind:0/3/10002 fetches)
  into `nmp-core` — those are module concerns (D0). The impl PR for M2 must register those
  interests from `nmp-nip01`/`nmp-nip02` or the demo app, not from the kernel actor directly.

The compiler is **in-memory v1** by design. The mailbox cache is the existing `HashMap<String, AuthorRelayList>` (`crates/nmp-core/src/kernel/mod.rs:313`); it just gets a new consumer.
