# 07 — Subscription planner: Interest → CompiledPlan → wire

> Status: **SHIPS**. Audience: builders + agents.
> The planner crate ships at `crates/nmp-planner/`. The
> `SubscriptionLifecycle` (07/14) drives it; the legacy kernel REQ emitters do
> not yet consume its output — see the reality-check at the end of [10 — Outbox
> routing (NIP-65)](10-outbox-routing.md).

## Why a compiler, not a string formatter

A naive Nostr client formats one REQ per `subscribe()` call at the call site.
Three things go wrong: (1) 1000 timeline avatars become 1000 profile REQs;
(2) a kind:10002 arriving late never re-routes the open subscription;
(3) the app hand-rolls relay fan-out and leaks the old REQ on follow-list
change. NMP turns "what a consumer wants alive on the wire" into a
`LogicalInterest` and runs a pure 4-stage **compiler** that produces a
`CompiledPlan`. Recompilation is safe on every trigger because the wire-emitter
diffs plans — a no-op recompile is zero wire effect
(`docs/design/subscription-compilation/recompilation.md` §4).

`LogicalInterest` (`crates/nmp-planner/src/interest.rs`) is *not* a Nostr
filter. It carries: `id` (registry-assigned, survives recompile),
`scope` (`ActiveAccount` / `Account` / `Global`), `shape`
(`InterestShape`), `hints`, and `lifecycle`
(`Tailing` / `OneShot` / `BoundedTime`). `InterestShape`
(`crates/nmp-planner/src/interest.rs`) mirrors a filter but uses sorted containers
(`BTreeSet` / `BTreeMap`) so equality and hashing are deterministic — that
determinism is what makes plan-id stable.

## The 4-stage pipeline

`SubscriptionCompiler` (`crates/nmp-planner/src/compiler/mod.rs`)
runs (design: `docs/design/subscription-compilation/compiler.md` §3):

1. **Resolve** — each author/`#p`/address → mailboxes via `MailboxCache`
   (`crates/nmp-planner/src/compiler/mailbox.rs`). Direction is decided by shape:
   `authors` → Outbox (write relays); `#p` → Inbox (read relays);
   `addresses` → Outbox keyed on `NaddrCoord::pubkey`; none → active-account
   read relays.
2. **Indexer fallback** — authors with no known mailbox route to the
   configured indexer set (read-only; never for publish, per D3). Surfaced as
   `RoutingSource::UserConfigured(Indexer)` — a sub-category of lane 4, **not**
   a fifth lane (`crates/nmp-planner/src/plan.rs`).
3. **Per-relay merge** — group by relay URL; `lattice::merge()`
   (`crates/nmp-planner/src/lattice/mod.rs`) folds compatible shapes. Author sets are
   partitioned per relay (`crates/nmp-planner/src/compiler/partition/mod.rs`): each
   relay's sub-shape carries only the authors that declared it.
4. **Plan-id binding** — `compute_plan_id` content-addresses
   `(sorted interests, referenced mailbox snapshot, lattice version)`. Same
   inputs → same `plan_id`; the platform reads it for diagnostic continuity.

The 9 merge-lattice rules live in `crates/nmp-planner/src/lattice/rules.rs` — equality
on `kinds`/`since`/`until`/`lifecycle`/`relay_pin`, union on
`tags`/`event_ids`/`addresses`, refuse on any `limit`. **Rule 9** (`relay_pin`)
is the third routing lane (see Case E below). Read `compiler.md` §3.3 for the
rule semantics; do not re-derive them in app code.

## Deliverable: CompiledPlan for "5 followed authors × 2 relays each"

Five authors A–E. A,B,C declare `{relay1, relay2}`; D,E declare
`{relay2, relay3}`. A notes feed declares primary kind `{1}` with reposts
enabled; the protocol adapter compiles that into acquisition kinds `{1,6}`.
One tailing interest reaches the planner. The union of write relays is
`{relay1, relay2, relay3}`; each relay's sub-shape carries only its declared
author subset, merged into one REQ:

```
CompiledPlan { plan_id: "p-9c3a…", per_relay: BTreeMap {
  "wss://relay1" → RelayPlan {
      role_tags: { Nip65 },
      sub_shapes: [ SubShape { authors:{A,B,C}, kinds:{1,6},
                               lifecycle:Tailing, hash:"1f0a…" } ] },  # 1 REQ
  "wss://relay2" → RelayPlan {
      role_tags: { Nip65 },
      sub_shapes: [ SubShape { authors:{A,B,C,D,E}, kinds:{1,6},
                               lifecycle:Tailing, hash:"7b22…" } ] },  # 1 REQ
  "wss://relay3" → RelayPlan {
      role_tags: { Nip65 },
      sub_shapes: [ SubShape { authors:{D,E}, kinds:{1,6},
                               lifecycle:Tailing, hash:"c4e1…" } ] },  # 1 REQ
} }
```

Three relays, three REQs total — not five, not fifteen. relay2 serves five
authors in one merged sub-shape because Rule 1 (`kinds` equal) and Rule 2
(`tags` same dimensions) passed and authors unioned. This is the M2 audit
gate's central assertion (`docs/design/subscription-compilation/tests.md`
§9.2 Assertion 2).

## Deliverable: recompilation triggers

The compiler is idempotent over `(interest_set, mailbox_snapshot,
indexer_set, user_config)`. Triggers fan in from ingest, the view registry,
and session/config (full table:
`docs/design/subscription-compilation/recompilation.md` §4.0). Highest-signal
subset:

| ID | Source | Trigger | What it carries | M-scope |
|---|---|---|---|---|
| A1 | ingest | `Nip65Arrived` | a kind:10002 landed for a pubkey | M2 |
| A2 | interest owner | `OwnerOpened` | interests or child interests just registered | M2 |
| A3 | interest owner | `OwnerClosed` | warmth grace expired; interests dropped | M2 |
| A4 | session | `ActiveAccountChanged` | account switch | M8 |
| A5 | relay worker | `RelayReconnected` | socket re-established (replay only) | M2 |
| A6 | operator | `InvalidateCompile` | external force-recompile (the one public action dispatch) | M2 |
| — | source owner | `SourceReducerChanged` | an internal source reducer replaced its materialized child-interest set | #2092 |

Non-triggers (do **not** recompile): an EVENT arriving on an existing REQ; an
EOSE on a one-shot (lifecycle closes it); a refcount delta not crossing 0↔1;
RTT/byte counters. This keeps recompile cadence tied to routing change, not
event throughput.

## Internal source reducers and dependent interests

The planner consumes **materialized** `LogicalInterest`s. It does not know what
"following", "mute list", "follow pack", or any other protocol/app source
means. Dynamic feeds are expressed one layer above the planner as an internal
source reducer:

```
source interest -> deterministic reducer -> materialized child interests
```

App feed code opens the feed-shaped typed-session helper from ADR-0076, backed
by `FeedParams`: primary content kinds plus a closed `FeedSourceExpr` source
expression. Protocol/defaults code owns the NIP-specific reducer, replaces the
full child-interest set when the source changes, and sends those children
through the same registry and compiler as ordinary claims. Component/read-model
dependencies use the same lifecycle: avatars claim profiles, target previews
claim events/addresses, and pointer feeds claim the targets they explicitly
render.

Current default reducers include active-account follows, NIP-51 people-list
members, and the active account's public mute-list `p` tags. They differ only in
the protocol-owned reducer; the planner still receives materialized interests.

Three invariants matter:

- Empty reduced output fails closed. It never becomes wildcard `authors`,
  `ids`, or tag filters.
- Source replacement withdraws stale children before installing the new set,
  so removed authors/tags/ids close on the wire.
- `ActiveAccountChanged` re-runs active-account sources; children derived from
  the old account do not survive the switch.

## Deliverable: worked example — source reducer change → CLOSE/REQ deltas

An active-user follow feed is open. The app declared primary kinds `{1}` and
the NIP-02/defaults reducer has reduced the active account's current source
event to child author interest `{A,B,C}`. Mailbox cache is seeded
A→relay1, B→relay2, C→relay3. Plan v1 opens REQs on `{relay1, relay2,
relay3}`.

A fresher source event arrives and reduces to `{A,B,D}` (D→relay4). The source
owner replaces the materialized child set, the compiler sees the new
`LogicalInterest` set, and the wire-emitter diffs:

```
plan v1: relay1{A}  relay2{B}  relay3{C}
plan v2: relay1{A}  relay2{B}              relay4{D}
diff   : —          —          CLOSE c…r3  REQ c…r4
```

Exactly two wire frames: `CLOSE` on the relay3 slice (C dropped) and `REQ` on
relay4 (D added). **Zero churn** on relay1 (A unchanged) or relay2 (B
unchanged). The feed handle is not destroyed. A stale source event rejected by
replaceable-supersession fires no source replacement.

If D's kind:10002 is unknown, D routes according to the current content policy:
configured app relays if present, otherwise `unroutable_authors` diagnostics.
The concurrent kind:10002 fetch later fires `Nip65Arrived`, recompiling D onto
its declared relay — a *second* delta the M2 NIP-65 gate covers separately.

## Callout: `relay_pin` / Case E (the third routing lane)

Some protocols (NIP-29 relay-based groups, future closed-relay NIPs) require
a subscription to go to **one specific host** regardless of any author's
NIP-65 mailboxes. `InterestShape::relay_pin: Option<RelayUrl>`
(`interest.rs:114-140`) is the generic, protocol-agnostic carrier. When
`Some(host)`:

- **Case E** (`planner/compiler/partition/case_e_relay_pinned.rs:46-71`)
  short-circuits the four-lane dispatch entirely — no `MailboxCache` lookup,
  no `request_probe`, no indexer fallback. Routing source is
  `UserConfigured(Debug)` so the diagnostics surface stays at four lanes.
- **Rule 9** (`planner/lattice/rules.rs:160-162`): two shapes merge only if
  `relay_pin` is *identical*. `None` does **not** absorb `Some(_)` (unlike
  Rule 1's wildcard `kinds`): mixing pinned + unpinned would leak pinned
  content to other relays or narrow the unpinned scope. Same-host pins
  coalesce normally — Rule 2's tag-value union collapses many per-room `h`
  filters into one per-host REQ (the "h-tag coalesce" the lane is named
  after).

`relay_pin` is never serialized onto the wire; the relay receives only the
regular filter. The kernel grows zero protocol nouns — `nmp-nip29` is a pure
consumer (ADR-0012; `docs/decisions/0012-relay-pinned-interest-and-third-routing-lane.md`).

## Anti-patterns

1. **Assuming 1 filter == 1 REQ.** The compiler merges N interests into
   M ≤ N sub-shapes per relay. Counting REQs by interest count is wrong;
   read `plan.per_relay[..].sub_shapes`.
2. **Passing relay URLs to view-open APIs.** There is no relay field on a
   view spec. The only surfaces that name a relay are the audited publish
   override, diagnostics (read-only), and user config — never a view.
3. **Hand-rolled dedup in app code.** "1000 avatars → 1 profile REQ" is the
   compiler's job (claim-merge via Rule 1/2). App-side de-dup re-introduces
   the leak the planner exists to extinguish.
4. **Forgetting to close interests on view destruction.** Interests are
   refcounted by `InterestId`; a view that never drops its claim keeps a
   tailing REQ alive past the warmth grace.
5. **Emitting plan-id churn on trivial recompile.** Plan-id hashes only
   *referenced* mailboxes. Hashing the whole cache (or mutating a `SubShape`
   without `recompute_hash`, `plan.rs:112-114`) churns plan-ids and breaks
   the wire-emitter diff.

See also: [06 — Reactivity contract (D8)](06-reactivity-contract.md) ·
[08 — EventStore + insert invariants + GC](08-eventstore.md) ·
[10 — Outbox routing (NIP-65)](10-outbox-routing.md) ·
[14 — Subscription lifecycle + relay manager + NIP-42](14-relay-manager.md) ·
[20 — Adding a new protocol module (`nmp-nip29` as reference)](20-new-protocol-module.md)
