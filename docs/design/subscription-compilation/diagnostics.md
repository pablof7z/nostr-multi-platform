# Subscription Compilation §5 + §8 — Four-Lane Diagnostics

> Parent: `docs/design/subscription-compilation.md`.
> Read first: ADR-0007 (`docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md`) — every record here extends ADR-0007 types; it does not replace them.

The compiler's routing decisions are the most subtle correctness surface in the M2 milestone. They are also the easiest to silently get wrong (`docs/design/ndk-applesauce-lessons.md` §3, "automatic behaviour also needs strong tests"). Diagnostics make the four sources of relay knowledge legible — separately, never collapsed.

## 5.0 The four lanes

Per `docs/design/ndk-applesauce-lessons.md` §4 (lines 39–46) and `docs/aim.md` §6 doctrine 10 ("provenance preserved"), the four relay-fact lanes are:

1. **NIP-65** — a pubkey's declared relay preferences (kind:10002).
2. **Hint** — relay URLs embedded in events or NIP-19 pointers (`e`/`a` tag third slot, `nevent`'s relay vector, etc.).
3. **Provenance** — relays we have actually observed an event arriving from.
4. **User-configured** — local-policy relays added by the user/operator, plus the kernel-configured indexer fallback set.

Each lane is its own record stream. They never merge into a single "relays" field — that collapse is exactly the bug `docs/design/ndk-applesauce-lessons.md` §4 line 46 forbids. They may be displayed side-by-side in a diagnostic view; the actor stores them apart.

This is structurally enforced: there is no `Vec<RelayUrl>` field on any compiler output type. Every relay-bearing field carries a `lane: RelayFactLane` discriminator.

```rust
// crates/nmp-core/src/kernel/diagnostics/lanes.rs (proposed)

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelayFactLane {
    Nip65,
    Hint,
    Provenance,
    UserConfigured,
}
```

## 5.1 Per-lane record schemas

Each lane has one record type. All four are exposed to the platform via the existing ADR-0007 `ViewBatch` lane (low-cadence, coalesced to 1–4 Hz per ADR-0007 "How status crosses the bridge"). They feed into the diagnostics screen, not into normal product UI.

### Lane 1 — `Nip65RelayFact`

```rust
pub struct Nip65RelayFact {
    pub pubkey: Pubkey,
    pub relay_url: RelayUrl,
    pub roles: Nip65Roles,                    // read | write | both
    pub kind10002_event_id: EventId,           // provenance of the kind:10002
    pub kind10002_created_at: UnixSeconds,
    pub kind10002_seen_from: Vec<RelayUrl>,    // which relays delivered it
    pub freshness: FreshnessHint,              // recent / hours_old / days_old / never_verified
}

pub struct Nip65Roles {
    pub read: bool,
    pub write: bool,
}
```

Emitted whenever `ingest_relay_list` (`crates/nmp-core/src/kernel/ingest.rs:209-233`) replaces a mailbox entry. One record per `(pubkey, relay_url)` pair; an author with 4 declared relays produces 4 records on each update.

### Lane 2 — `HintRelayFact`

```rust
pub struct HintRelayFact {
    pub relay_url: RelayUrl,
    pub source: HintSource,
    pub freshness_ms: u64,                     // monotonic from observation
    pub recently_succeeded: bool,              // last attempt produced ≥1 EVENT
}

pub enum HintSource {
    EventTag    { event_id: EventId, tag: TagKey, position: u8 },
    Nip19       { pointer: String /* nevent1.../naddr1... */ },
    UserConfig  { config_path: String },        // for hints injected via config
}
```

Emitted by the pointer loader (post-M2; for M2 the field exists but is rarely populated — only `e`/`a`-tag third-slot hints from thread-view hydration fill it). Per-event hints are de-duplicated; an event whose `e` tag contains a hint URL produces one `HintRelayFact` per (relay_url, source) pair.

### Lane 3 — `ProvenanceRelayFact`

```rust
pub struct ProvenanceRelayFact {
    pub relay_url: RelayUrl,
    pub event_id: EventId,
    pub seen_at_ms: u64,
    pub wire_sub_id: String,                   // which REQ delivered it
    pub kind: u32,
    pub author: Pubkey,
}
```

Emitted by `handle_event` (`crates/nmp-core/src/kernel/ingest.rs:134-164`) for every EVENT arrival. This is the highest-cardinality lane and the only one where coalescing matters at the ADR-0007 boundary: the platform diagnostic view consumes a summarised projection (`ProvenanceSummary` per author or per event), not the raw fact stream.

### Lane 4 — `UserConfiguredRelayFact`

```rust
pub struct UserConfiguredRelayFact {
    pub relay_url: RelayUrl,
    pub category: UserConfiguredCategory,
    pub generation: u64,                       // config version; matches Trigger::*Changed
    pub added_at_ms: u64,
}

pub enum UserConfiguredCategory {
    AccountRead,                                // user's own read relays (overrides NIP-65 read)
    AccountWrite,                               // user's own write relays
    Indexer,                                    // kernel indexer set member
    Debug,                                      // operator-injected for testing
}
```

Emitted on `Trigger::UserConfiguredRelaysChanged` / `Trigger::IndexerSetChanged`. Low-cardinality, low-cadence.

## 5.2 What the compiler may *combine*; what stays distinct

The compiler may *use* facts from multiple lanes to compute a routing decision; it may **not** present them as one. Concretely:

- **Routing decision** (`AuthorRouting.source` from [compiler.md](compiler.md) §3.1): records *which lane* the relay set was derived from. Single-valued; one of `Nip65 | Hint | Indexer | UserConfigured`. The author may have facts in three lanes; the compiler picks one, says so, and the other lane records remain visible.
- **`RelayPlan.role_tags`** is a `BTreeSet<RoutingSource>` because a single relay may be in the plan for multiple reasons (e.g. NIP-65 for author A + user-configured fallback for everyone). The set discriminates, it does not collapse.
- **The platform diagnostic view** receives all four lanes as separate `ViewBatch` records. The UI may render them in one screen with four side-by-side columns, but the data path is four lanes.

A test (§9) asserts that no compiler output type has a field of type `Vec<RelayUrl>` without an adjacent `RelayFactLane`. That is the structural enforcement.

## 5.3 Lane interactions

The lanes inform each other through these well-defined hooks:

- `Provenance → NIP-65 hint refresh.` If we observe many `ProvenanceRelayFact { relay_url: R, author: A }` records but no `Nip65RelayFact { pubkey: A, relay_url: R }`, the operator diagnostic can suggest "we are receiving A's events from R but A has not declared R; their kind:10002 may be stale." This is a future operator-UI affordance, not a behaviour.
- `Hint → planner suggestion.` `HintRelayFact` with `recently_succeeded: true` may surface in the diagnostic view as "you might want to add this to your indexer set." Again, not automatic.
- `User-configured` is the **only** lane the compiler treats as authoritative-by-policy (the user said so). Open question 5 in the parent index resolves the augment-vs-override precedence between NIP-65 and user-configured for the active account.

The lesson the four-lane discipline preserves: routing is **derivable but contested** evidence. Collapsing the lanes loses information; preserving them lets the diagnostic answer "why did we route this REQ to that relay?" months after the decision.

---

# §8 — Reverse-relay-coverage diagnostic view

> The inverse question. For any relay we are talking to, *whose* timeline does it serve?

This is one specific `ViewModule` that consumes the four-lane fact streams plus the compiler's `RelayPlan`s and produces a per-relay summary.

## 8.1 Spec, payload, dependencies

```rust
pub struct RelayCoverageSpec {
    pub relay_url: RelayUrl,
}

pub struct RelayCoveragePayload {
    pub relay_url: RelayUrl,
    pub serving_authors: u32,
    pub author_examples: Vec<Pubkey>,      // first N (configurable, default 16)
    pub by_lane: ByLaneCounts,
    pub wire_sub_count: u32,
    pub last_event_at_ms: Option<u64>,
    pub provenance_count_last_minute: u32,
}

pub struct ByLaneCounts {
    pub nip65: u32,             // authors for whom relay is in their NIP-65 set
    pub hint: u32,              // authors for whom we routed here via hints
    pub user_configured: u32,   // authors served via user-config
    pub indexer_fallback: u32,  // authors with no mailbox, served via indexer
}

// `ViewModule::dependencies` returns:
//   - Mailbox cache updates touching any author in our timeline
//   - RelayPlan updates touching `relay_url`
//   - Provenance facts on `relay_url` (rate-limited; only the count, not individual events)
```

## 8.2 Implementation outline

The view's `reduce` consumes three input streams:

1. `Nip65RelayFact` records — increments/decrements `by_lane.nip65` per (relay_url, pubkey) membership.
2. `CompiledPlan` re-emissions — every plan recompile produces a `(plan_id, relay_url) → authors` projection that this view subscribes to. The compiler exposes this projection as `RelayAuthorCoverage` in the kernel's projection cache (per `docs/design/reactivity/view-deltas-and-projections.md`).
3. `ProvenanceRelayFact` records — feeds the rolling 60-second counter for `provenance_count_last_minute`.

This is the M2 exit-gate diagnostic listed in [`docs/plan/m2-subscription-compilation.md`](../../plan/m2-subscription-compilation.md) ("Reverse-relay-coverage view for diagnostics: 'this relay is serving N authors of our timeline.'").

## 8.3 Cardinality and emission cadence

One `RelayCoverageSpec`/relay → ≤ N records, where N is the number of relays currently in the planner's union of `RelayPlan`s. For typical Nostr usage that is in the low tens; rendering all of them on one diagnostic screen is fine.

Emission cadence follows ADR-0007's diagnostic-view rule: material-transition immediately, otherwise 1–4 Hz. The provenance counter ticks every second; the `by_lane` counts only emit on `CompiledPlan` recompiles or new mailbox arrivals.

## 8.4 Why it lives in diagnostics, not in product UI

Per `docs/aim.md` §4.4 ("the developer does not pick relays per operation; the framework does") and ADR-0007's domain-of-diagnostics separation, end-user product UIs do not show "relay X is serving 12 authors." That is operator/debug surface. Normal apps consume the `LogicalInterestStatus` summaries; `RelayCoveragePayload` is for the diagnostics screen (proof iOS app screenshot in `docs/perf/m2/outbox-routing.md` per [`docs/plan/m2-subscription-compilation.md`](../../plan/m2-subscription-compilation.md)).
