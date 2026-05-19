# Relay roles — Indexer + AppRelay design

> **Status:** Design draft. No code yet.
> **ADR:** `docs/decisions/0021-relay-roles-indexer-and-app-relay.md`.
> **Date:** 2026-05-18.
> **Research:** `docs/research/relay-lifecycle-and-pools.md` (existing
> NMP relay architecture); the cross-library research at
> `docs/research/ndk/` and `docs/research/applesauce/` directories.
> Two flat-file research reports (`ndk-app-relay-model.md` and
> `applesauce-app-relay-model.md`) were produced during this design
> session but were wiped by a subsequent repository operation before
> this doc was committed; recreate from the parent chat transcript or
> rerun the agents if needed for traceability.

## 0. Relationship to existing NMP primitives

NMP already has a worker-level `RelayRole` enum
(`crates/nmp-core/src/relay.rs:57`) with variants
`Content / Indexer / Wallet`. **This is NOT what ADR-0021's
`RoutingSource::Indexer` and `RoutingSource::AppRelay` propose.** The
two operate at different abstraction levels:

| Concept | Layer | Authority for | Source-of-truth comment |
|---|---|---|---|
| `RelayRole` | Worker / transport | Connection-pool bucketing, relay-health rows, NIP-42 driver state, `wire_subs` diagnostic surface. | *"Not a routing source (T105). The actual wire target is the resolved `OutboundMessage::relay_url`. `RelayRole` only buckets relay-health rows…"* (`relay.rs:49`) |
| `OutboundMessage::relay_url` | Worker / transport | The actual wire target (routing authority since T105). | *"Resolved wire target. The transport dials this URL."* (`relay.rs:118`) |
| `RoutingSource` | Planner | **Why** a given relay was selected for a given filter/event. Diagnostic lane decoration. | `planner/plan.rs::RoutingSource` |
| `RelayPlan::role_tags` | Planner output | Set of `RoutingSource`s explaining each per-relay plan row. | ADR-0012 + ADR-0020 + this ADR. |

The existing `BOOTSTRAP_DISCOVERY_RELAYS` constant
(`relay.rs:27`) hardcodes
`["wss://relay.damus.io", "wss://nos.lol"]` as the cold-start seeds
for the `Content` and `Indexer` transport lanes respectively. The
doctrine comment explicitly states these are *"NOT a routing default
— content/profile/thread REQs and publishes target the resolved
`OutboundMessage::relay_url`, not this. Used only so the very first
kind:10002 discovery fetch has a relay to dial before any NIP-65 list
is cached."* (`relay.rs:81–85`).

### What this ADR changes about that

This ADR does not modify the worker `RelayRole` enum. It adds new
`RoutingSource` variants at the planner layer. The mapping between the
two layers in v1:

| `RoutingSource` (planner lane) | Frames travel on `RelayRole` (worker lane) |
|---|---|
| `Nip65 { direction }` | `Content` |
| `Hint` | `Content` |
| `Provenance` | `Content` |
| `UserConfigured(AccountRead/Write/Debug)` | `Content` |
| `ClassRouted { class: Search, … }` | `Content` (or a future `Search` lane if NIP-50 fanout cost warrants splitting) |
| `ClassRouted { class: Draft, … }` | `Content` |
| `ClassRouted { class: Wiki, … }` | `Content` |
| `Indexer` (this ADR) | **`Indexer`** — the existing lane absorbs the new always-on traffic |
| `AppRelay` (this ADR) | `Content` (v1); a future `AppRelay` worker lane is possible but not v1 |

The two `Indexer` names line up at v1 deliberately: traffic the planner
labels as `RoutingSource::Indexer` (universal-data kind subscriptions
and publishes) dials via the worker's existing `RelayRole::Indexer`
socket pool, which is what `nos.lol` (the cold-start seed) already
serves. AppRelay traffic shares the `Content` lane in v1 — splitting
it out is a future optimisation if the seven-lane diagnostic shows
fan-in collisions.

### Migration implication for `BOOTSTRAP_DISCOVERY_RELAYS`

The hardcoded constant becomes the **default operator value** for the
two new config fields:

```rust
KernelConfig {
    default_indexer_relays: vec!["wss://nos.lol".into()],     // was lane-2 bootstrap
    default_app_relays:     vec!["wss://relay.damus.io".into()], // was lane-1 bootstrap
    // (Indexer recommended default still aligns with NIP-51 PR #1985's
    //  `wss://purplepag.es/` — see ADR-0021 decision (5). Operators
    //  override at construction.)
    // ...
}
```

The existing `BOOTSTRAP_DISCOVERY_RELAYS` constant survives as the
**fallback when operator config is empty** — same role it plays today,
just promoted into being one input to a layered config rather than the
sole source of truth.

## 1. Goals

1. NMP supports two operator-configured relay roles distinct from
   user-author NIP-65 mailboxes:
   - **Indexer relays** — always-on for kinds in
     `INDEXER_KINDS = {0} ∪ {3} ∪ {10000..=19999}`, both reads and writes.
     Source: operator default ∪ user's published kind:10086.
   - **AppRelay relays** — per-author substitutive fallback when an
     author has no known NIP-65 mailbox. Source: operator default ∪
     user-settings override.
2. Every relay URL in every plan carries its `RoutingSource` lane(s);
   no global "all connected relays" pool exists. Operations specify
   their lane scope explicitly.
3. The kernel never silently merges signer-supplied relays.
4. The diagnostic UI shows seven lanes plus the blocked-relay
   subtractive filter; every routing decision is operator-visible.

## 2. Non-goals

- Inventing a new NIP for AppRelay user preferences. Client-local
  settings are the v1 mechanism.
- Subtraction semantics for kind:10086 (NIP-51 PR #1985 is additive
  only). Users who want to remove an operator default do so via
  client settings.
- NIP-66 relay-liveness filtering. Defer to a future ADR.
- Cross-device sync of AppRelay preferences.

## 3. Type surface

### 3.1 `RoutingSource` final shape (post-ADR-0020 + ADR-0021)

```rust
// nmp-core::planner::plan

#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum RoutingSource {
    /// Lane 1 — per-author NIP-65 outbox/inbox.
    Nip65 { direction: Direction },
    /// Lane 2 — relay hint from event tag.
    Hint,
    /// Lane 3 — provenance from prior event.
    Provenance,
    /// Lane 4 — user-configured (active-account read/write, debug).
    /// `Indexer` is REMOVED from this enum and promoted to lane 6.
    UserConfigured(UserConfiguredCategory),
    /// Lane 5 — NIP-51 class routing (ADR-0020).
    ClassRouted { class: EventClass, via: ClassRoutingPath },
    /// Lane 6 — operator-configured indexer relays.
    /// ALWAYS-ON for kind:0, kind:3, kind:10000–19999. R+W symmetric.
    Indexer,
    /// Lane 7 — operator-configured app relays.
    /// Per-author substitutive fallback when NIP-65 mailbox unknown.
    AppRelay,
}

#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum UserConfiguredCategory {
    AccountRead,
    AccountWrite,
    /// Operator-injected for debug/testing only (single-relay injection).
    Debug,
    // NB: `Indexer` variant removed — now `RoutingSource::Indexer`.
}

#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum ClassRoutingPath {
    /// Personal NIP-51 list (active-account context).
    Personal,
    /// Publisher-keyed NIP-51 list (kind:10102, per-author).
    PublisherKeyed { author: Pubkey },
}
```

### 3.2 Kind-gate constant

```rust
// nmp-core::routing::indexer

/// Kinds whose every read/write plan unions the indexer set.
/// Per ADR-0021 §Decision (3): broader than NIP-51 PR #1985's literal
/// "kinds 0 and 10002" — applies the same kind:10086 event shape to
/// the wider universal-data range.
pub const INDEXER_KINDS_RANGE: std::ops::RangeInclusive<u32> = 10000..=19999;
pub const INDEXER_KINDS_DISCRETE: &[u32] = &[0, 3];

#[inline]
pub fn is_indexer_kind(kind: u32) -> bool {
    INDEXER_KINDS_DISCRETE.contains(&kind) || INDEXER_KINDS_RANGE.contains(&kind)
}
```

### 3.3 Extended `OutboxResolver`

Layered onto ADR-0020's already-extended trait:

```rust
pub trait OutboxResolver: Send + Sync {
    // ─── existing (from M2) ───
    fn write_relays(&self, author: &Pubkey) -> Vec<RelayUrl>;
    fn read_relays(&self, author: &Pubkey) -> Vec<RelayUrl>;

    // ─── from ADR-0020 ───
    fn class_relays_personal(&self, class: &EventClass) -> Option<Vec<RelayUrl>>;
    fn class_relays_for_author(
        &self,
        class: &EventClass,
        author: &Pubkey,
    ) -> Option<Vec<RelayUrl>>;
    fn blocked_relays(&self) -> std::collections::BTreeSet<RelayUrl>;

    // ─── NEW (this ADR) ───

    /// Indexer relays — `operator_default ∪ user_kind_10086`.
    /// Always returns at least the operator default. Empty only if
    /// the operator explicitly configured no defaults.
    fn indexer_relays(&self) -> Vec<RelayUrl>;

    /// AppRelay relays — `operator_default ∪ user_settings_override`.
    /// Always returns at least the operator default. Empty only if
    /// the operator explicitly configured no defaults.
    fn app_relays(&self) -> Vec<RelayUrl>;

    /// `true` iff the kernel has a NIP-65 mailbox cached for the
    /// author. Drives the AppRelay per-author fallback decision in
    /// `case_i_app_relay`. Returns `false` for unknown mailboxes
    /// AND for empty mailboxes (an author who explicitly published
    /// kind:10002 with no relay tags is rare and routes through
    /// AppRelay).
    fn has_mailbox(&self, author: &Pubkey) -> bool;
}
```

### 3.4 Kernel-init config

```rust
// nmp-core::kernel::config

pub struct KernelConfig {
    // ... existing fields ...

    /// Operator default Indexer relays. Recommended seed value:
    /// `vec!["wss://purplepag.es/".into()]` per NIP-51 PR #1985.
    /// Unioned with user's kind:10086 at runtime.
    pub default_indexer_relays: Vec<RelayUrl>,

    /// Operator default AppRelay relays. Used per-author when the
    /// author has no NIP-65 mailbox. Recommended: at least one entry
    /// for cold-start reliability (e.g., the app's preferred host).
    pub default_app_relays: Vec<RelayUrl>,

    /// (ADR-0020) Per-class fallback when user has no NIP-51 list of
    /// the corresponding class. Conceptually adjacent to AppRelay but
    /// scoped to class-routed events specifically.
    pub default_relay_lists: DefaultRelayLists,
}
```

### 3.5 Settings FFI surface

```rust
// nmp-core::settings

/// Read-write view onto operator-overridable relay sets. Persisted
/// in client-local store; never published to Nostr (kind:10086 is
/// handled separately via the normal publish path).
pub trait RelaySettings {
    fn indexer_relays_local_override(&self) -> Option<Vec<RelayUrl>>;
    fn set_indexer_relays_local_override(&self, relays: Option<Vec<RelayUrl>>);

    fn app_relays_local_override(&self) -> Option<Vec<RelayUrl>>;
    fn set_app_relays_local_override(&self, relays: Option<Vec<RelayUrl>>);

    /// One-call read for a settings UI: returns every relay the kernel
    /// considers active with its role(s). Mirrors what the diagnostic
    /// pane would show but in a stable settings shape.
    fn enumerate_active_relays(&self) -> Vec<ActiveRelayEntry>;
}

pub struct ActiveRelayEntry {
    pub url: RelayUrl,
    pub roles: Vec<RoutingSource>,
    pub source: RelaySource,
}

pub enum RelaySource {
    OperatorDefault,
    UserPublished { kind: u32 },     // e.g. kind:10086, kind:10002
    ClientSettings,                  // user-set via app settings UI
    SignerSuggested,                 // surfaced but NOT auto-merged
}
```

The settings view is what powers the user's stated UX: *"settings could
show a list of relays and their roles."* `enumerate_active_relays` is
the read; `set_*_local_override` are the writes.

## 4. Planner integration

### 4.1 New partition cases

Two new cases land in `nmp-core::planner::compiler::partition`:

```
existing order (ADR-0020):
  case_a_authors
  case_b_addresses
  case_c_p_tags
  case_d_no_author
  case_e_relay_pinned        (NIP-29)
  case_g_class_routed        (ADR-0020 — NIP-51 specialized)

new (this ADR):
  case_h_indexer             (always-on for INDEXER_KINDS)
  case_i_app_relay           (per-author NIP-65-miss fallback)
```

### 4.2 `case_h_indexer` — always-on for INDEXER_KINDS

```
for each LogicalInterest I in the compile set:
  if I.shape.kinds.any(|k| is_indexer_kind(*k)):
    let indexers = resolver.indexer_relays()
    for relay in indexers:
      RelayPlan[relay].interests.push(I)
      RelayPlan[relay].role_tags.insert(RoutingSource::Indexer)
```

This runs **after** `case_g_class_routed` so that class-routed
plans (e.g., a kind:10007 search REQ that already went to the user's
search relays per ADR-0020) also union the indexer set. Indexer is
purely additive — never substitutes for or filters out other lanes.

Publish-side symmetry: the publish engine applies the same gate.
When `PublishEngine` dispatches an event with `is_indexer_kind(event.kind)`,
it unions `resolver.indexer_relays()` into the target list. Closes the
applesauce R+W asymmetry.

### 4.3 `case_i_app_relay` — per-author NIP-65-miss fallback

```
for each per-author lane L in the compiled plan (from case_a_authors):
  if !resolver.has_mailbox(&L.author):
    let app_relays = resolver.app_relays()
    for relay in app_relays:
      RelayPlan[relay].interests.push(L.interest_with_author_pinned(L.author))
      RelayPlan[relay].role_tags.insert(RoutingSource::AppRelay)
    L.mark_substituted()  // this author has no Nip65 lane now
```

Granularity: per ADR-0021 decision (6), the substitution happens
per-author within an interest. For `authors=[a, b, c]` where `a` has
NIP-65 but `b`/`c` don't, `a`'s lane stays on outbox; `b` and `c`
get AppRelay lanes carrying author-pinned filters.

Lifetime: per ADR-0021 decision (7), AppRelay lanes persist for the
whole session. When `b` later publishes kind:10002, the next
recompilation observes `has_mailbox(b) == true`, `case_i_app_relay`
skips `b`, and `b` graduates to outbox routing on the next plan
update. The AppRelay subscription for `b` closes naturally as the
recompiled plan drops it.

### 4.4 No-author interests

For `case_d_no_author` interests (kind-only filters, e.g.,
"subscribe to kind:1 everywhere"), AppRelay does NOT apply — there's
no author to fallback for. Such interests already route through
indexer (if their kind is in `INDEXER_KINDS`) or through
`active_account_read_relays`. No change to `case_d`.

### 4.5 Merge lattice — new rule

**Rule 11 — `Indexer` and `AppRelay` lanes never affect mergeability.**
Two shapes that would otherwise merge (per Rules 1–10) still merge if
one is indexer-routed and the other isn't, *as long as the wire
filter is identical*. The lane tag is a per-(filter, relay) decoration,
not a filter dimension. This keeps wire REQ frame count minimal.

Practically: a kind:10002 read for author A (NIP-65 route) and a
kind:10002 read for author B (also NIP-65 route, also unions indexer)
can produce a single merged REQ to a relay that's in both A's outbox
and the indexer set — with `role_tags = {Nip65, Indexer}`. The
diagnostic UI shows both lanes; the wire only sees one frame.

## 5. Fact stream — kind:10086

Kind:10086 (per NIP-51 PR #1985) is a replaceable list with `relay`
tags. The wiring:

1. Register `10086` with the `nmp-nip51` decoder
   (`crates/nmp-nip51/src/kinds.rs::ALL_KINDS`).
2. Subscribe to kind:10086 for the active account as part of the
   boot sequence, alongside kind:10002 (NIP-65) and the ADR-0020 lists
   (10006/10007/10013/10050/10102).
3. Project decoded `relay` tags into the resolver's indexer state:
   ```rust
   pub struct IndexerFacts {
       pub operator_default: Vec<RelayUrl>,
       pub user_kind_10086: Vec<RelayUrl>,
       pub user_client_override: Option<Vec<RelayUrl>>,
   }
   impl IndexerFacts {
       pub fn active_set(&self) -> Vec<RelayUrl> {
           // Client override wins entirely when present (per ADR-0021
           // §Decision (5)+(8) discussion of subtraction).
           // Otherwise union operator default with kind:10086.
           if let Some(override_) = &self.user_client_override {
               return override_.clone();
           }
           let mut set: BTreeSet<RelayUrl> =
               self.operator_default.iter().cloned().collect();
           set.extend(self.user_kind_10086.iter().cloned());
           set.into_iter().collect()
       }
   }
   ```
4. `resolver.indexer_relays()` returns `IndexerFacts::active_set()`.

The same shape (minus the kind:10086 layer) applies to AppRelay
state:

```rust
pub struct AppRelayFacts {
    pub operator_default: Vec<RelayUrl>,
    pub user_client_override: Option<Vec<RelayUrl>>,
}
impl AppRelayFacts {
    pub fn active_set(&self) -> Vec<RelayUrl> {
        self.user_client_override
            .clone()
            .unwrap_or_else(|| self.operator_default.clone())
    }
}
```

## 6. Diagnostic discipline

Seven routing lanes:

1. NIP-65 (per-author outbox/inbox)
2. Hint (event-tag relay hints)
3. Provenance (prior-event relay observation)
4. UserConfigured (AccountRead / AccountWrite / Debug)
5. ClassRouted (ADR-0020 — NIP-51 specialized)
6. **Indexer (this ADR — always-on for INDEXER_KINDS)**
7. **AppRelay (this ADR — per-author NIP-65-miss fallback)**

Plus the subtractive global filter:

- **Blocked** (kind:10006) — applied post-planning, removes relays.

The diagnostic-doc update lands in P3 of the joint ADR-0020 + ADR-0021
rollout (single PR introducing both new lanes simultaneously).

### Operator-visible source enumeration

Every `ActiveRelayEntry` (§3.5) records *which source* added it.
The diagnostic pane shows: URL | roles | source. For example:

| URL | Roles | Source |
|---|---|---|
| `wss://purplepag.es/` | Indexer | OperatorDefault |
| `wss://my-app.example/` | AppRelay | OperatorDefault |
| `wss://alice-prefers.example/` | Indexer | UserPublished(kind:10086) |
| `wss://relay.damus.io` | Nip65(Outbox), Nip65(Inbox) | UserPublished(kind:10002) |
| `wss://signer-suggested.example/` | — (none active) | SignerSuggested |

The last row demonstrates per ADR-0021 §Decision (10): the signer
*suggested* a relay; the kernel records it but never auto-merges.
The user can promote it via settings (changes `Source` to
`ClientSettings`).

## 7. Algorithm primitives (ported from applesauce)

Per `docs/research/SYNTHESIS-app-relays.md` §4, applesauce has a set
of well-tested coverage-selection primitives we should adopt in Rust
idiom:

```rust
// nmp-core::planner::selection

/// Greedy coverage-maximising relay selection with a per-user cap.
/// Port of applesauce's `selectOptimalRelays(users, opts)`
/// (`packages/relay-pool/src/helpers/relay-selection.ts:14`).
pub fn select_optimal_relays(
    candidates: &OutboxMap,
    opts: SelectionOpts,
) -> Vec<(RelayUrl, Vec<Pubkey>)> { /* ... */ }

pub struct SelectionOpts {
    pub max_connections: Option<usize>,
    pub max_relays_per_user: Option<usize>,
    /// Optional relay score function (higher = preferred).
    pub score: Option<Box<dyn Fn(&RelayUrl) -> f64 + Send + Sync>>,
}

/// Port of `groupPubkeysByRelay(pointers)` — produces the OutboxMap
/// shape. The most reusable primitive.
pub fn group_pubkeys_by_relay(
    pointers: &[ProfilePointer],
) -> OutboxMap { /* ... */ }

pub type OutboxMap = std::collections::HashMap<RelayUrl, Vec<Pubkey>>;
```

These don't change ADR-0021's semantic decisions — they're
implementation primitives the planner uses inside `case_a_authors`
and the new `case_i_app_relay`. Direct algorithmic port from
applesauce; we benchmark NMP's behavior against applesauce's
test suite during P4.

## 8. Migration / rollout plan

This ADR's phases interleave with ADR-0020's. The combined sequence:

| Phase | Deliverable                                                                                              | Gate                          |
|-------|----------------------------------------------------------------------------------------------------------|-------------------------------|
| P1    | `EventClass` + `from_kind` + `RoutingFamily` (ADR-0020 §3.1).                                            | Existing tests still pass.    |
| P2    | `InterestShape::{search, class_hint}` + Rule 10 + ADR-0020 partition `case_g_class_routed`.              | Determinism gate green.       |
| **P2.5** | **`RoutingSource::Indexer` + `::AppRelay` promoted; `UserConfiguredCategory::Indexer` removed; `is_indexer_kind` const.** | **Variant migration green.** |
| P3    | NIP-51 routing facts substrate slice (ADR-0020 + kind:10086 from this ADR) + extended resolver trait.    | Five-→seven-lane diagnostic asserts. |
| **P3.5** | **`case_h_indexer` + `case_i_app_relay` partition cases; `has_mailbox` resolver method; symmetric R+W indexer in `PublishEngine`.** | **Lazy 10102 + per-author AppRelay lifecycle tests green.** |
| P4    | `SearchQuery` FFI + cache scan + relay fanout (ADR-0020 §3.5).                                           | Integration tests green.      |
| **P4.5** | **`RelaySettings` FFI surface (this ADR §3.5) + `enumerate_active_relays`.**                            | **Chirp's settings UI consumes it.** |
| P5    | `PublishTarget::Auto` upgrade + blocked-relay filter + fail-loud (ADR-0020).                             | M11.5 exit gate.              |

P2.5 and P3.5 are this ADR's hinges. P4.5 lights up the user-facing
settings surface.

## 9. FFI / app-developer ergonomics

```swift
// Kernel construction with operator defaults:
let kernel = Kernel(config: .init(
    defaultIndexerRelays: ["wss://purplepag.es/"],   // NIP-51 PR #1985 default
    defaultAppRelays: ["wss://relay.my-app.example/"],
    defaultRelayLists: .init(
        search: ["wss://search.nos.lol/"],
        drafts: [],                                    // app doesn't ship draft default
        wiki: []
    )
))

// Settings UI reads:
let entries = kernel.settings.enumerateActiveRelays()
// → [
//     (wss://purplepag.es/, [Indexer], OperatorDefault),
//     (wss://my-app.example/, [AppRelay], OperatorDefault),
//     (wss://alice-prefers.example/, [Indexer], UserPublished(kind:10086)),
//     (wss://relay.damus.io, [Nip65(Outbox), Nip65(Inbox)], UserPublished(kind:10002)),
//   ]

// User adds a custom indexer in app settings:
kernel.settings.setIndexerRelaysLocalOverride([
    "wss://purplepag.es/",
    "wss://my-personal-indexer.example/"
])
// Now user's client override wins; kind:10086 layer is ignored for
// this session (see §5 — client override is total, not additive).

// Publishing a kind:0 profile update:
kernel.publish(event: profileEvent, target: .auto)
// Kernel: kind 0 → is_indexer_kind(0) = true
//                → union(write_relays(self), indexer_relays())
//                → subtract blocked_relays() → dispatch.
// Profile lands on the user's outbox AND every indexer.
```

The "cold-start works" promise: a fresh user with no NIP-65 + no
kind:10086 still has fully-functional reads (AppRelay covers
NIP-65-missing authors; operator-default indexer covers
universal-data kinds) and fully-functional writes (publishes hit
operator-default indexer for replaceable kinds, AppRelay for events
they author).

## 10. Test surface (M-gate criteria)

- **Unit: `is_indexer_kind`.** Asserts kind:0, kind:3, kind:10000,
  kind:10002, kind:10006, kind:10007, kind:10013, kind:10050, kind:10086,
  kind:10101, kind:10102, kind:19999 are all in scope. Asserts
  kind:1, kind:4, kind:14, kind:20000, kind:30023, kind:30818 are
  NOT in scope.
- **Unit: indexer-set source chain.** With operator default `[X]` and
  user kind:10086 `[Y]` and no client override, active set is
  `{X, Y}`. With client override `[Z]`, active set is `{Z}` (overrides
  both layers).
- **Unit: AppRelay per-author partition.** Interest with
  `authors=[a, b]`, `has_mailbox(a)=true`, `has_mailbox(b)=false`.
  Plan partitions a → outbox lane (NIP-65), b → AppRelay lane.
- **Unit: AppRelay session lifetime.** After b publishes kind:10002,
  next recompilation routes b → outbox; AppRelay lane for b closes.
- **Symmetric R+W indexer.** `PublishEngine` dispatching kind:0
  targets `write_relays(self) ∪ indexer_relays()`. Dispatching
  kind:1 targets `write_relays(self)` only (kind:1 not in
  `INDEXER_KINDS`). Both subject to blocked-relay filter.
- **No silent signer merge.** `Kernel::attach_signer(s)` where
  `s.relays()` returns `[Z]` does NOT add `Z` to any routing lane.
  `enumerate_active_relays` shows `Z` with `Source::SignerSuggested`
  and empty `roles`.
- **Anti-#175 invariant.** For every compiled plan, every
  `RelayPlan` has `!role_tags.is_empty()` — assertion at compile end.
- **Diagnostic seven-lane fixture.** One example per lane plus the
  blocked-relay subtraction; cross-references ADR-0020's five-lane
  fixture and extends it.
- **Indexer load test.** Session with active subscriptions on
  10 kinds in `INDEXER_KINDS` and a 3-entry indexer set: assert
  total indexer-bound REQs ≤ 10 (one per kind, merged across kinds
  when filter shape allows per Rule 11).

## 11. Open questions surfaced during design

These are not v1-blocking but worth tracking:

1. **Should `RelaySource::SignerSuggested` entries auto-expire** if
   the user doesn't promote them within N days? Otherwise the
   settings UI accumulates stale signer suggestions across signer
   reconnects.
2. **NIP-66 liveness integration.** When NIP-66 lands, both Indexer
   and AppRelay sets should be filtered against liveness data. Own
   ADR.
3. **Cross-device sync of client-settings overrides.** v1 keeps
   them client-local; if a "settings sync" feature lands (kind:30078
   per NIP-78, or similar), this design needs an update so override
   semantics survive device transitions cleanly.
4. **Subtraction primitives.** Both kind:10086 and AppRelay
   client-settings are positive-list shapes. If users routinely
   need "remove operator-default X without replacing the entire
   list," a NIP for additive/subtractive deltas would help. Out of
   scope here.
