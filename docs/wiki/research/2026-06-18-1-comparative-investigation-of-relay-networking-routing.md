---
type: research-record
date: 2026-06-18
session: 1670fcb8-f275-498c-975b-8bd912331ded
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/1670fcb8-f275-498c-975b-8bd912331ded.jsonl
source_lines: 13932-14257
agent_attribution: Codex research agent
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Comparative investigation of relay networking/routing separation across applesauce, NDK, rust-nostr, and Bitcoin Core; verdict that NMP's crate-boundaries design conflates routing and pool lifecycle and should split nmp-relay-pool into nmp-router + nmp-network"
captured_at: 2026-06-18T05:11:36Z
---

Comparative investigation of relay networking/routing separation across applesauce, NDK, rust-nostr, and Bitcoin Core; verdict that NMP's crate-boundaries design conflates routing and pool lifecycle and should split nmp-relay-pool into nmp-router + nmp-network

---

User: [Agent task result: Agent "Codex research — applesauce/NDK relay pool + routing patterns" completed]
Confirmed: `send_msg_to` was a deprecated method (the test still has it). The current API is `client.send_msg(msg).to(urls).await` or `client.send_msg(msg).broadcast().await`. The pool's `send_msg` ALWAYS requires a URL set — it never has a "no urls = decide for me" mode. Now I have all evidence. Writing the report.

---

# Research Report: Relay Networking vs Routing Separation in Mature Nostr Libraries

## TL;DR

Three mature Nostr libraries have evolved toward the same shape: **dumb dispatcher pool below, smart routing engine above, transport trait below the pool**. rust-nostr makes this most explicit and most testable. The current NMP `crate-boundaries.md` design (two new crates: `nmp-transport` for sockets, `nmp-relay-pool` for routing + pool lifecycle + per-kind dispatch) **conflates two distinct concepts** that rust-nostr separates: a `RelayPool` that knows nothing, and a `Gossip`/`OutboxRouter` that decides where things go. The NMP design should consider splitting `nmp-relay-pool` further, or at minimum naming the seam explicitly inside that crate.

A secondary finding: the user's "per-relay filter execution strategy" maps cleanly to the `authors`-partitioning pattern that applesauce and rust-nostr both implement. Per-relay `since`/cursor customization is **not in prior art**; if NMP wants it, that's a novel cursor primitive, distinct from routing.

---

## 1. applesauce — already exhaustively researched

The existing NMP doc at `/Users/pablofernandez/Work/nostr-multi-platform/docs/research/applesauce-app-relay-model.md` (536 lines) covers this completely with verbatim citations. Key facts that are load-bearing for the crate split:

- **`RelayPool` is connection-only.** Constructor at `/tmp/applesauce-research/packages/relay/src/pool.ts` accepts only `RelayOptions` (timeouts, reconnect policy, NIP-42 auth). There is no `defaultRelays` argument. Relays are created on demand by URL via `pool.relay(url)`.
- **Routing lives in `applesauce-core/helpers/relay-selection.ts`, NOT in the pool.** `groupPubkeysByRelay` produces `OutboxMap = Record<RelayUrl, ProfilePointer[]>`. `createFilterMap` (line 138) projects that into per-relay filters by replacing the `authors` field — exactly the per-relay filter customization pattern.
- **Pool exposes both `subscription(relays, filter)` (uniform fan-out) and `subscriptionMap(filterMap)` (per-relay filters).** The pool itself doesn't know which it's getting; both end up calling `RelayGroup#subscription` per relay.
- **Outbox routing is opt-in.** App developer must call `pool.outboxSubscription` or compose `OutboxModel` with `includeFallbackRelays`. The "blast every relay" path is the default.

What this teaches NMP: applesauce has effectively three layers — `Relay` (single socket Rx observable), `RelayPool` (collection + grouping), and `applesauce-core/helpers/relay-selection` (pure routing). The pool is genuinely dumb. The user's intent matches applesauce here.

The one applesauce design choice NMP should reject: **opt-in correctness**. Make outbox-aware routing the default path, not the alternative method.

## 2. NDK — already exhaustively researched

The existing NMP doc at `/Users/pablofernandez/Work/nostr-multi-platform/docs/research/ndk-app-relay-model.md` covers this. Key delta from applesauce relevant to crate split:

- **NDK has two pools, both held by the god-object `NDK`.** `pool` (main) and `outboxPool` (metadata discovery). Both are `NDKPool` instances; the difference is policy, not type.
- **`OutboxTracker` is a separate object from the pool** (`/tmp/.../ndk/core/src/outbox/tracker.ts` per the existing research doc). It owns the kind:10002 LRU cache. The pool consumes the tracker indirectly via `calculateRelaySetFromEvent`.
- **`NDKRelaySet` is a stateful object lifecycled per subscription**, not pure data. It contains the per-sub union of relays produced by the routing calculation.
- **Routing decisions are in `core/src/relay/sets/calculate.ts`**, not in the pool. The pool just dispatches.

What NDK does wrong that NMP must invert: **god-set behavior**. NDK's zap bug (#175) — `pool.relays.keys()` serializes every connected URL into a header — is the direct result of a pool that doesn't carry per-relay role/purpose. Every relay must know why it's connected.

What NDK does right that NMP should adopt: **outbox is on by default**. The opt-in opposite would re-create applesauce's mistake.

## 3. rust-nostr — the strongest prior art for NMP's split

This is where the report adds new evidence. rust-nostr (`/tmp/rust-nostr-research/sdk/`) has the cleanest three-layer decomposition in the wild.

### 3.1 The three layers

```
/tmp/rust-nostr-research/sdk/src/transport/websocket.rs:64
  → trait WebSocketTransport { fn connect(url) -> (sink, stream); }
  Pure socket. No frames, no routing. 8 lines of trait surface.

/tmp/rust-nostr-research/sdk/src/pool/mod.rs:42
  → pub(crate) struct RelayPool { ... }
  Crate-private dumb dispatcher. Takes pre-resolved per-relay frames or
  filters. Returns Output<T>. Zero routing knowledge.

/tmp/rust-nostr-research/sdk/src/client/gossip/mod.rs:50
  → pub(super) struct Gossip { store, resolver, semaphore, refresher }
  Lives in the Client layer (not the pool). Resolves PublicKey → relay set
  using NIP-65/NIP-17 data from a pluggable `NostrGossip` store trait.
```

The key visibility-modifier observation: `RelayPool` is `pub(crate)`. **The pool is never exposed as a public API of the SDK.** Users interact with `Client`, which composes `Gossip + RelayPool + Transport`. The pool has no business being a public type — once routing is separated, the pool surface is too narrow to need direct access.

### 3.2 The pool's actual signature (the smoking gun)

`/tmp/rust-nostr-research/sdk/src/pool/mod.rs:431-480` (`send_msg`):

```rust
pub(crate) async fn send_msg(
    &self,
    set: HashSet<RelayUrl>,                       // pre-resolved by caller
    msg: ClientMessage<'_>,
    wait_until_sent: Option<Duration>,
) -> Result<Output<()>, Error> {
    if set.is_empty() {
        return Err(Error::NoRelaysSpecified);     // pool NEVER decides
    }
    // ... iterates relays, calls relay.send_msg(msg) per URL
}
```

`/tmp/rust-nostr-research/sdk/src/pool/mod.rs:552-612` (`subscribe`):

```rust
pub(crate) async fn subscribe(
    &self,
    filters: HashMap<RelayUrl, Vec<Filter>>,      // per-relay filters!
    id: Option<SubscriptionId>,
    auto_close: Option<SubscribeAutoCloseOptions>,
) -> Result<Output<SubscriptionId>, Error> {
    if filters.is_empty() {
        return Err(Error::NoRelaysSpecified);
    }
    // ... for each (url, filter) in filters: relay.subscribe(filter).with_id(id)
}
```

**The pool's subscribe signature is `HashMap<RelayUrl, Vec<Filter>>`** — per-relay filters are the contract. The pool does not pretend it understands "subscribe to author X." It accepts a precomputed plan.

`pool.send_msg(msg)` with no URLs **does not exist**. The deprecated `client.send_event_to(urls, ev)` is now `client.send_event(ev).to(urls).await` (`/tmp/rust-nostr-research/sdk/src/client/api/send_event.rs:1184`). And `client.send_event(ev).await` with no `.to()`/`.broadcast()` defaults to **gossip routing**, falling back to `pool.write_relay_urls()` only when gossip isn't configured.

### 3.3 Where routing actually happens

`/tmp/rust-nostr-research/sdk/src/client/api/send_event.rs:381-435` is the routing decision point:

```rust
let urls: HashSet<RelayUrl> = match (self.policy, self.client.gossip()) {
    (None | Some(OverwritePolicy::ToNip65), Some(gossip)) =>
        gossip_prepare_urls(self.client, gossip, self.event, false).await?,
    (Some(OverwritePolicy::ToNip17), Some(gossip)) =>
        gossip_prepare_urls(self.client, gossip, self.event, true).await?,
    (Some(OverwritePolicy::ToNip17 | OverwritePolicy::ToNip65), None) =>
        return Err(Error::GossipNotConfigured),
    (Some(OverwritePolicy::To(list)), _) => { /* explicit URLs */ }
    (Some(OverwritePolicy::Broadcast), _) | (None, None) =>
        self.client.pool().write_relay_urls().await,
};
Ok(self.client.pool().send_event(urls, self.event, ...).await?)
```

This is a clean two-step: (1) resolve URLs (gossip or explicit), (2) hand pre-resolved URLs to the pool. The pool is invoked exactly once at the end with a fully-resolved set. **There is no path in which the pool decides routing.**

### 3.4 The per-relay filter compiler

`/tmp/rust-nostr-research/sdk/src/client/gossip/resolver.rs:93-180` (`break_down_filter`) is the function `crate-boundaries.md` describes as "projects to per-relay filter":

```rust
// For a filter with authors=[A,B,C], find each author's write relays,
// then for each relay, emit a NEW filter with authors = only those who write there.
let mut map: HashMap<RelayUrl, Filter> = HashMap::with_capacity(outbox.len());
for (relay, pk_set) in outbox.into_iter() {
    let mut new_filter: Filter = filter.clone();
    new_filter.authors = Some(pk_set);              // <-- per-relay authors
    map.insert(relay, new_filter);
}
Ok(BrokenDownFilters::Filters(map))
```

The per-relay customization is **only on the `authors` field**. Every other field (kinds, since, until, limit, generic_tags) is identical across relays. See section 5 below for what this means for NMP.

The wrapper `gossip_break_down_filters` (`/tmp/rust-nostr-research/sdk/src/client/gossip/updater.rs:445`) handles multiple input filters and unions them into the final `HashMap<RelayUrl, Vec<Filter>>` that goes to the pool.

### 3.5 The Gossip store is itself pluggable

`/tmp/rust-nostr-research/sdk/src/client/gossip/mod.rs:50-66`:

```rust
pub(super) struct Gossip {
    store: Arc<dyn NostrGossip>,                    // trait-injected
    resolver: GossipRelayResolver,
    semaphore: GossipSemaphore,
    refresher: GossipBackgroundRefresher,
}
```

`NostrGossip` is a trait from a separate crate (`nostr-gossip`). The default backend is `nostr-gossip-memory`. The user can supply persistent backends. This is the inversion that NMP's `MailboxCache` trait achieves — but rust-nostr has shipped it and exercised it.

### 3.6 What this teaches NMP

The single most important finding of this report: **rust-nostr's `RelayPool` does what NMP's `crate-boundaries.md` says `nmp-relay-pool` should do, but it deliberately does NOT include `OutboxRouter`.** That responsibility lives one layer up, in the client/composition layer.

NMP's current Layer 2 (`nmp-relay-pool`) combines four things:
1. Per-kind `OutboxRouter` dispatch table
2. `MailboxCache` impl
3. Pool lifecycle (connect/disconnect/reconnect/socket budget)
4. The NIP-65 `publish_relay_list` `ActionModule`

rust-nostr's equivalents split as:
- (1) and (2) live in `Client + Gossip` (NMP's would-be Layer 3 or higher)
- (3) lives in `RelayPool` (matches NMP Layer 2 partially)
- (4) does not exist (rust-nostr has no equivalent of NMP's NIP crate concept)

The user's stated design intent matches rust-nostr exactly: **"The router determines where events go or are REQed from."** The router is *not* the pool. The router *consumes* the pool. The fact that `crate-boundaries.md` puts `OutboxRouter` *inside* `nmp-relay-pool` is the unexamined choice the brief asked us to flag.

## 4. Bitcoin Core — the same shape, validated under hostile conditions

Bitcoin Core's P2P stack has spent 15 years being attacked. Its current shape is the lesson NMP should treat as load-bearing.

`/tmp/bitcoin-research/src/net.h:1070` defines `class CConnman` — owns sockets, address selection, connection budget, ban list. ~3500 LOC of pure connectivity.

`/tmp/bitcoin-research/src/net.h:1026` defines `class NetEventsInterface` — the callback surface `CConnman` invokes for `InitializeNode`, `FinalizeNode`, `ProcessMessages`, `SendMessages`. **`CConnman` knows nothing about transactions, blocks, headers, or any P2P message semantics.** It just calls back to the registered `m_msgproc` (a `NetEventsInterface*`) when bytes arrive.

`/tmp/bitcoin-research/src/net_processing.h:76`:

```cpp
class PeerManager : public CValidationInterface, public NetEventsInterface
{
public:
    static std::unique_ptr<PeerManager> make(
        CConnman& connman, AddrMan& addrman, BanMan* banman,
        ChainstateManager& chainman, CTxMemPool& pool,
        node::Warnings& warnings, Options opts);
    // ...
};
```

`PeerManager` lives in `net_processing.cpp` and is the message router. It consumes `CConnman` (to learn about peers and send messages), `AddrMan` (peer database, separate object), `ChainstateManager`/`CTxMemPool` (the things peers exchange). **It tells `CConnman` where to send each message** via `connman.PushMessage(peer, msg)`. `CConnman` never decides "this peer wants this block" — it's a postman.

The three-object split is identical to rust-nostr's three-layer split:
| Bitcoin Core | rust-nostr | NMP analog |
|---|---|---|
| `CConnman` (sockets) | `WebSocketTransport` + `RelayPool` | `nmp-transport` + (pool half of `nmp-relay-pool`) |
| `AddrMan` (peer database) | `NostrGossip` store + `Gossip` | (cache half of `nmp-relay-pool`) |
| `PeerManager` (routing + protocol) | `Client::send_event/subscribe` + `gossip_break_down_filter` | (router half of `nmp-relay-pool`) |

Bitcoin Core's net/net_processing separation was the result of a multi-year refactor (the most famous PRs were #15141 and #19398 era, the "Make CConnman not include validation" cleanup). The original monolithic design had three load-bearing problems that NMP's current `nmp-relay-pool` design risks replicating:

1. **Net code couldn't be unit-tested without spinning up validation.** Routing is policy; transport is mechanics; mixing them means tests must mock policy to test mechanics.
2. **Adding a new P2P message type required edits in both places.** Equivalent risk for NMP: every new NIP that needs custom routing has to know about pool internals.
3. **Bug surface area scaled with the product of (peers × message types).** The split makes bugs scale with `peers + message types`.

The lesson for NMP: even if the practical implementation puts `OutboxRouter` and the pool in the same crate for v1 expedience, **the trait boundary must be enforced inside the crate** so it can be extracted later without API churn.

## 5. The "per-relay filter execution strategy" — partially novel

The user wrote: *"per-relay a filter execution strategy is determined."* This phrase compresses two distinct ideas. The report disambiguates them.

### 5.1 What IS in prior art: per-relay `authors` partitioning

Both rust-nostr and applesauce do this. The pattern is:

- Input: one logical filter, e.g. `{ kinds: [1], authors: [A, B, C], since: T }`.
- Routing: A writes to relays r1, r2; B writes to r1, r3; C writes to r3, r4.
- Output: per-relay filters where `authors` is the subset writing to that relay.
  - r1 gets `{ kinds: [1], authors: [A, B], since: T }`
  - r2 gets `{ kinds: [1], authors: [A], since: T }`
  - r3 gets `{ kinds: [1], authors: [B, C], since: T }`
  - r4 gets `{ kinds: [1], authors: [C], since: T }`

Citations:
- rust-nostr: `/tmp/rust-nostr-research/sdk/src/client/gossip/resolver.rs:167-178` (the `new_filter.authors = Some(pk_set)` line).
- applesauce: `/tmp/applesauce-research/packages/core/src/helpers/relay-selection.ts:138-145` (`createFilterMap`).
- NDK: per the existing research file, `calculateRelaySetsFromFilter` is kind-agnostic and groups authors by their outbox relays then fans filters per relay. Same pattern.

This is **wire-correctness routing**: each relay only sees the authors it's responsible for. It's the floor of competence for an outbox-aware client.

### 5.2 What is NOT in prior art: per-relay `since`/cursor customization

The user's stronger example — "relay A already has events up to T_A from this author, so use `since: T_A` on the REQ to A" — does NOT appear in any of the three libraries inspected.

- rust-nostr's `events_tracker.rs:8` is just `LruCache<EventId, ()>` — a global dedup set. No per-(relay, author, kind) bookkeeping.
- applesauce's filter map only varies `authors`. `since`/`until` are immutable per logical request.
- NDK has no equivalent. The existing research doc at `relay-lifecycle-and-pools.md` does not mention per-relay cursors either.

**The closest existing concept is NIP-77 negentropy reconciliation** (which NMP already has a crate for). Negentropy is *different*: it's a round-trip protocol that lets two parties discover their event-set delta. It does not produce a per-relay `since` cursor — it produces an event ID delta.

**Cursor concern vs routing concern.** If NMP wants per-relay since cursors, the report's framing is:

- This is a **cursor concern**, not a routing concern. State is per (relay × author × kind) tuple, written when events arrive, read when subscriptions compile.
- It lives below the router: the router asks the cursor store "what's the most recent event I have from author A on relay r1?" and stamps the resulting `since: T_A` into the filter for r1.
- The router does not own the cursor data; the storage layer (NMP's `nmp-store`) does.
- The compiler that emits per-relay filters consults the cursor store as one input among several (the others being NIP-65, hints, AppRelay fallback).

This is a **novel NMP primitive**. No prior art exists to copy. If it's in scope, it should be its own ADR — the cursor primitive is orthogonal to the routing seam, and conflating them in `nmp-relay-pool` would re-create the kitchen-sink problem the workspace just split out of.

## 6. Critique of NMP's two-crate design

The user brief says "we are designing two new crates" (networking + routing). The current `crate-boundaries.md` puts them at Layer 1 (`nmp-transport`) and Layer 2 (`nmp-relay-pool`). The prior art above says: that's the right minimum, but the second crate contains at least two distinct concerns that rust-nostr and Bitcoin Core separate.

**Concretely, `nmp-relay-pool` as currently scoped contains:**

A. Pool lifecycle: per-URL `Relay` actor, reconnect/backoff/jitter, AUTH re-do on reconnect, sub replay, publish replay. *Genuinely the pool.*

B. `OutboxRouter` impl: per-kind dispatch table, `RoutingRule` registry, the routing decision logic. *In rust-nostr, this lives in `Client::send_event` + `gossip_break_down_filter`. In Bitcoin Core, this lives in `PeerManager`.*

C. `MailboxCache` impl: kind:10002 cache. *In rust-nostr, this is `NostrGossip` store; in Bitcoin Core, this is `AddrMan`. Both are separate objects.*

D. NIP-65 `publish_relay_list` ActionModule. *App-level action; in rust-nostr this would live in the client's action dispatcher.*

The brief's "two crates" framing groups A with (B+C+D). rust-nostr groups (B+C+D) and leaves A alone. Bitcoin Core puts each of A, B, C in its own object (`CConnman`, `PeerManager`, `AddrMan`).

**Recommended decisions for NMP:**

1. **Keep the two-crate top-level split, but enforce trait boundaries inside `nmp-relay-pool`.** The `OutboxRouter` trait must be a true seam: routing impls are constructor-injected, not hardcoded. The `MailboxCache` trait must allow non-mailbox implementations (NIP-17 DM relay cache is the immediate proof). If a future extraction is needed, the visible API does not change.

2. **The pool sub-module's public API must not return `RelayPool` to upstream callers.** Match rust-nostr's `pub(crate)` discipline. The pool is an implementation detail of routing. The user-facing API is `OutboxRouter::route_publish` and `OutboxRouter::route_subscription`, both of which take logical intents and return `RoutedRelaySet`. The pool's `send_msg(set, msg)` and `subscribe(map, id)` are the internal contract between router and dispatcher.

3. **Treat the per-kind dispatch table as the substrate's only "router knowledge."** Per `crate-boundaries.md` §3.4, NIP crates register `RoutingRule` impls at composition time. The router walks the rules; the pool dispatches the resulting set. This matches Bitcoin Core's split where `PeerManager` knows about transaction relay AND `CConnman` does not.

4. **Make outbox routing the default path.** rust-nostr makes gossip the default when configured; applesauce makes broadcast the default. NDK makes outbox the default but conflates it with a god-set. NMP's discipline (per the existing synthesis doc) is "correct-by-default, every source operator-visible." Match rust-nostr's default-on-when-configured shape; refuse to allow callers to bypass the router.

5. **Treat per-relay `since` cursors as orthogonal.** If they're a v1 goal, file a separate ADR. The cursor data lives in `nmp-store`. The router consults the store when compiling per-relay filters. The pool does not know cursors exist.

6. **The NIP-65 `publish_relay_list` action does not need to live in `nmp-relay-pool`.** Per the user's bias toward small NIP crates: this could be its own `nmp-nip65` crate after all (which `crate-boundaries.md` recently deleted with the argument "too thin"). The counter-argument: rust-nostr's gossip layer publishes NIP-65 lists from inside the gossip module because the gossip module already owns the kind:10002 cache write path. If `nmp-relay-pool` owns the cache, the action lives with it. If a future extraction moves the cache, the action follows it. This is a *consequence* of decision (1), not an independent choice.

## 7. The single key insight per library

The brief asked: for each library, what does it do right, what does it do wrong, what's the one key insight NMP should steal.

**applesauce.** Right: pure-data routing primitives (`OutboxMap = Record<RelayUrl, ProfilePointer[]>`), pool is connection-only, per-relay filters via `createFilterMap`. Wrong: outbox is opt-in (default is "blast every relay"). **Steal:** model the routing decision as data, not as an object with methods. NMP's `RoutedRelaySet` per `crate-boundaries.md` §3.2 is exactly this; reinforce it.

**NDK.** Right: outbox is on by default; refresh hook re-fans subscriptions on new kind:10002. Wrong: god-set behavior — `pool.relays.keys()` is the only way to ask "what's connected?" and that set is used for everything, causing #175 zap bug. **Steal:** mark every relay in the pool with its lane(s). NMP's per-`RelayRole` worker design today is a coarse version of this; the routing-rule registry refines it.

**rust-nostr.** Right: three-layer split (transport / pool / gossip), pool is `pub(crate)`, `Gossip` store is trait-injected. Wrong: per-relay customization is only `authors`; no per-relay cursor concept (this is just an unfilled niche, not a bug). **Steal:** the `pub(crate) RelayPool` visibility discipline. The pool is not a public API. The router is.

**Bitcoin Core.** Right: `CConnman` ↔ `NetEventsInterface` callback interface lets the router (`PeerManager`) be tested without sockets and the pool be tested without protocol semantics. `AddrMan` is its own object. Wrong: not really wrong; the original unified design WAS wrong, and the split is the lesson. **Steal:** the callback-interface idiom for ingest. NMP's `EventIngestDispatcher` + `IngestParser` trait (`crate-boundaries.md` §4.2) is the analog; verify the parsers are invoked from the pool's frame-ingest path via the same callback discipline `CConnman`/`NetEventsInterface` use.

## 8. Files referenced

External (cloned for this report):
- `/tmp/rust-nostr-research/sdk/src/transport/websocket.rs:64` — `WebSocketTransport` trait
- `/tmp/rust-nostr-research/sdk/src/pool/mod.rs:42,431,552,614,740` — `RelayPool` visibility and method signatures
- `/tmp/rust-nostr-research/sdk/src/client/gossip/mod.rs:50` — `Gossip` struct
- `/tmp/rust-nostr-research/sdk/src/client/gossip/resolver.rs:93,167-178` — `break_down_filter`
- `/tmp/rust-nostr-research/sdk/src/client/gossip/updater.rs:445` — `gossip_break_down_filters`
- `/tmp/rust-nostr-research/sdk/src/client/api/send_event.rs:381-435` — routing decision point
- `/tmp/rust-nostr-research/sdk/src/client/api/subscribe.rs:55-67` — subscribe goes through `build_targets`
- `/tmp/rust-nostr-research/sdk/src/client/api/util.rs:11-28` — `build_targets` dispatches to gossip or fallback
- `/tmp/rust-nostr-research/sdk/src/events_tracker.rs:8` — global LRU, no per-relay cursors
- `/tmp/applesauce-research/packages/core/src/helpers/relay-selection.ts:138` — `createFilterMap`
- `/tmp/applesauce-research/packages/relay/src/pool.ts:185-228` — `subscription` vs `subscriptionMap` vs `outboxSubscription`
- `/tmp/bitcoin-research/src/net.h:1026,1070` — `NetEventsInterface` and `CConnman`
- `/tmp/bitcoin-research/src/net_processing.h:76,99` — `PeerManager` and its `make()` constructor

Existing NMP research files (do not re-read; foundation for this report):
- `/Users/pablofernandez/Work/nostr-multi-platform/docs/research/applesauce-app-relay-model.md`
- `/Users/pablofernandez/Work/nostr-multi-platform/docs/research/ndk-app-relay-model.md`
- `/Users/pablofernandez/Work/nostr-multi-platform/docs/research/relay-lifecycle-and-pools.md`
- `/Users/pablofernandez/Work/nostr-multi-platform/docs/research/SYNTHESIS-app-relays.md`
- `/Users/pablofernandez/Work/nostr-multi-platform/docs/architecture/crate-boundaries.md`

## 9. The one paragraph for the decision

If the user reads only one section: **rust-nostr is the strongest validation that the user's design intent is correct, and the strongest critique of the current `crate-boundaries.md`. The "two new crates" framing matches rust-nostr's `transport/` and `pool/` layout, but `crate-boundaries.md` then puts `OutboxRouter` *inside* `nmp-relay-pool`, while rust-nostr puts the equivalent (`Gossip`) one layer up in `Client`. Bitcoin Core's `CConnman` / `PeerManager` / `AddrMan` triplet confirms this is the load-bearing decomposition: connection lifecycle is one thing, message routing is a second thing, peer database is a third. Whether NMP ships them as one crate, two crates, or three is an implementation expedience question; the trait boundaries must reflect the three concepts regardless. The user's "per-relay filter execution strategy" matches the existing `authors`-partitioning pattern in rust-nostr and applesauce, but per-relay `since` cursors are NOT in prior art — that would be a novel NMP cursor primitive belonging in `nmp-store`, not in either of the new crates.**
