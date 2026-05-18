# Outbox Calculation Explorer + Relay Usefulness Diagnostics

**Status.** M11 design doc. Read-only — no code changes here.
**Doctrine.** D0 (no app nouns), D4 (single-writer per fact),
D5 (bounded snapshot), D7 (capability report, no policy), D8 (working-set
bounded, no per-event allocation).
**Companion.** `docs/research/relay-lifecycle-and-pools.md` §3 + §4.1 (the
G8/M11 gap this surface formalises).

The explorer is a kernel-side **diagnostic projection** that lets a builder
answer, for any live WebSocket: (a) **why** we connected, (b) **what** REQs
are running on it, (c) per-socket **metrics**, and (d) **usefulness** — how
often this relay was the *first source* of a stored event. The named
consumer for the first read-out is `ios/Chirp/`; the data model has no
Chirp-specific shape (D0).

---

## 1 — Load-bearing invariant: at most ONE WebSocket per URL

> **The kernel maintains at most one WebSocket per resolved relay URL. The
> URL is the primary key of the transport pool; `RoutingSource` /
> `RelayRole` / future bunker-transport / NIP-50-search labels are
> **aggregations** over URLs, never multiplexing keys.**

Justification:

* Many relays reject duplicate sockets from the same client (or RST one
  side under wire-stress). The kernel cannot afford to find out which
  relay does what — assume duplicate-rejection across the board.
* NDK's `NDKRelay` is URL-identified for the same reason (research notes:
  `docs/research/ndk/` cluster, §“NDKRelay identity”). The single
  WebSocket is a 5+ year battle-tested invariant we inherit.
* Wire cost: every duplicate socket pays the WS handshake, the AUTH
  handshake (NIP-42), one full mailbox-discovery roundtrip, and N×
  redundant EVENTs we'd then have to dedupe in the store anyway.

Implications baked into the data model below:

1. `RelayConnectionDiagnostic` is keyed by **URL only**, never by
   `(role, URL)`.
2. The `why_connected` field is an aggregate **set across all roles** for
   that URL — one `Attribution` entry per decision; all share one socket
   and one diagnostic row.
3. The "first source" novel-event counter is naturally per-URL — exactly
   one socket can be the first to deliver any given event. No risk of
   splitting usefulness across roles (T105 already enforces URL-keyed
   `relay_controls` at `crates/nmp-core/src/actor/relay_mgmt.rs:84`).
4. `RelayControl::role` (singular field at `actor/mod.rs:130`) becomes
   either a `SmallVec` aggregate **or** a diagnostic-only label — see
   follow-up task **F5** below.

---

## 2 — Data model

All shapes are private to `crates/nmp-core/`. Serialised via the existing
T103 envelope onto the `update_tx` channel under a new
`KernelUpdate::relay_diagnostics: Option<Vec<RelayConnectionDiagnostic>>`
field (Option-gated — see §4).

```rust
/// One per-URL diagnostic row. Keyed by relay URL; aggregates every
/// reason this URL is currently dialed. (§1 single-socket invariant.)
pub(super) struct RelayConnectionDiagnostic {
    pub url: String,
    /// Aggregate set: one entry per decision that touched this URL.
    /// Bounded — cap at 32 entries; further decisions increment
    /// `attribution_overflow_count`.
    pub why_connected: Vec<Attribution>,
    pub attribution_overflow_count: u32,
    /// Subs currently open on this socket (mirror of `wire_subs`
    /// filtered by URL — no new state, projection only).
    pub subs: Vec<WireSubDiagnostic>,
    pub metrics: RelayMetrics,
    pub usefulness: RelayUsefulness,
}

/// Why this URL was included in the resolved relay set.
pub(super) struct Attribution {
    pub decision: AttributionDecision,
    /// Bounded last-N (N = 16) — older entries are dropped, count
    /// preserved in `author_overflow_count`.
    pub authors: Vec<String>,  // hex Pubkey
    pub author_overflow_count: u32,
    /// Logical interest ids that flowed through this decision.
    pub interests: Vec<InterestId>,
    /// kind:10002 event ids that drove inclusion (bounded N = 4).
    pub source_event_ids: Vec<String>,
    /// Wall-clock time the attribution was first recorded.
    pub first_seen_ms: u128,
}

pub(super) enum AttributionDecision {
    /// Author's NIP-65 write relays — outbox direction (timeline,
    /// thread, profile read-fan-out). `outbox.rs::author_write_relays`.
    Nip65AuthorWrite,
    /// `#p`-tagged recipient's NIP-65 read relays — inbox direction
    /// (notifications, DMs). `outbox.rs::recipient_read_relays`.
    Nip65RecipientRead,
    /// `InterestShape::relay_pin` — NIP-29 group host, naddr hint, etc.
    /// Mirrors `planner::plan::RoutingSource::Hint`.
    RelayPin,
    /// Cold-start seed — `BOOTSTRAP_DISCOVERY_RELAYS` only,
    /// never a routing default once kind:10002 lands. NOT to be
    /// conflated with `Nip65AuthorWrite` (relay.rs §T105 comment).
    BootstrapDiscovery,
    /// User-configured (account read/write, indexer, debug). Mirrors
    /// `RoutingSource::UserConfigured(UserConfiguredCategory)`.
    UserConfigured,
    /// Provenance-pinned — relay was the first source of a previously
    /// stored event from this author. `RoutingSource::Provenance`.
    Provenance,
}

pub(super) struct WireSubDiagnostic {
    pub wire_id: String,
    pub filter_summary: String,
    pub state: String,             // "opening" | "live" | "auth_paused" | ...
    pub interest_id: Option<InterestId>,
    pub opened_at_ms: u128,
    pub eose_at_ms: Option<u128>,
    pub events_delivered: u32,     // bounded counter, saturating
}

pub(super) struct RelayMetrics {
    pub connection_state: String,  // mirrors RelayHealth::connection
    pub auth_state: String,        // NIP-42, ADR-0007 keys
    pub nip77_verdict: String,     // negentropy capability
    pub reconnect_count: u32,
    pub frames_rx: u64,
    pub events_rx: u64,
    pub eose_rx: u64,
    pub bytes_rx: u64,
    pub bytes_tx: u64,
    /// EOSE latency histogram: 8 power-of-two ms buckets
    /// (≤16, ≤64, ≤256, ≤1024, ≤4096, ≤16384, ≤65536, >65536).
    /// Hot-path cost: one `leading_zeros` + one AtomicU64 bump.
    pub eose_latency_buckets: [u64; 8],
    /// REQ→first-event latency, same 8-bucket layout as EOSE. Bumped
    /// once per sub at first EVENT arrival (the `last_event_at`
    /// transition from `None` on `WireSub`). Distinct from EOSE
    /// timing — for tail-latency / cold-relay analysis.
    pub first_event_latency_buckets: [u64; 8],
    /// Bounded — last 32 (notice, error). Older entries dropped FIFO.
    pub recent_notices: Vec<(u128, String)>,
    pub recent_errors: Vec<(u128, String)>,
    /// T114 dispatch-drop counter projected per-relay — see §5.
    pub dispatch_drops: u64,
}

pub(super) struct RelayUsefulness {
    /// Times this relay's frame triggered `InsertOutcome::Inserted`
    /// — i.e. it was the first source for that event id.
    pub novel_events: u64,
    /// Times this relay's frame triggered `InsertOutcome::Duplicate`.
    pub duplicate_events: u64,
    /// novel / (novel + duplicate); reported as Option<f32> when the
    /// denominator is non-zero. Snapshot-side computation; no kernel
    /// state.
    pub novelty_ratio: Option<f32>,
    /// Times an `InsertOutcome::Rejected` came in on this socket
    /// (sig-fail / NIP-40 expired / malformed).
    pub rejected_events: u64,
    /// `InsertOutcome::Replaced` — this socket delivered a newer
    /// replaceable that won.
    pub replaced_events: u64,
}
```

All counter widths are saturating `u64` (D8 — overflow is a numeric
artefact, never a wrap).

---

## 3 — D8 compliance: bounds, working-set, hot-path cost

| Concern                       | Bound                                            |
|-------------------------------|--------------------------------------------------|
| `why_connected` per URL       | ≤ 32 attributions; overflow into counter         |
| `authors` per attribution     | ≤ 16; overflow into `author_overflow_count`      |
| `source_event_ids` per attrib | ≤ 4; older dropped FIFO                          |
| `recent_notices` per URL      | ≤ 32; FIFO                                       |
| `recent_errors` per URL       | ≤ 32; FIFO                                       |
| `eose_latency_buckets`        | fixed 8 × u64 = 64 bytes                         |
| URLs in `relay_url_health`    | LRU-evict any URL with no socket for ≥ 5 min     |
| Snapshot inclusion (§4)       | Off by default; opt-in flag flips the field      |

**Per-event allocation budget on the hot path: 0.** The novel-vs-duplicate
counter bump at `kernel/ingest/timeline.rs:68` uses the existing
`InsertOutcome` match arm — no new allocations, two `AtomicU64::fetch_add`
calls (or `saturating_add` on owned state, since the kernel is single-
threaded under D4). Latency-bucket placement is one `u64::leading_zeros`
on the EOSE arm.

**Attribution writes are emit-time, not ingest-time.** The
`partition_authors_by_write_relays` caller (`kernel/ingest/timeline.rs:227`)
emits once per recompile; one allocation per `(url, authors)` pair, then
folded into the bounded structures. Ingest path is untouched.

---

## 4 — D0 / D5 compliance: where the projection lives

* **D0 (no app nouns).** Nothing in §2 references Chirp, Pulse, podcast,
  or any other shell. The shapes are URLs, pubkeys, interest ids, kinds,
  and outcomes — kernel vocabulary only.
* **D5 (snapshots bounded by what's open).** `relay_diagnostics` is
  Option-typed on `KernelUpdate`. Default is `None`; the production
  envelope keeps its existing footprint. The projection is opt-in via
  a future `KernelAction::EnableDiagnostics { explorer: true }` (D7 —
  capability report, never policy). Builder/Operator turns it on; the
  field flips to `Some(…)` and the bounded snapshot ships. Off again →
  back to `None` next emit. Working-set drop is immediate.
* **T103 envelope contract.** The wire shape adds one optional field on
  `KernelUpdate`; consumers ignoring it see no change (proto3-style
  forward compat). No new envelope variant.

---

## 5 — Implementation sketch: code seams

Mapped onto the current tree (lines verified against `master @ 2cd423a`):

1. **Attribution at emit time.** `partition_authors_by_write_relays`
   (`crates/nmp-core/src/kernel/outbox.rs:37`) already produces the
   `(url → authors)` map; the call site
   (`kernel/ingest/timeline.rs:227–246`) discards everything but the
   authors-per-URL slice. Replace the discard with a fold that writes
   one `Attribution { decision: Nip65AuthorWrite, authors, interests,
   source_event_ids: [self.author_relay_lists[a].event_id], ... }` into
   the kernel's per-URL bundle. Same pattern for `req_for_relay`
   (`kernel/requests/mod.rs:227`) — the universal REQ seam covers
   profile/thread/discovery emitters at zero hot-path cost.

2. **First-source counter on insert.** `kernel/ingest/timeline.rs:68`
   already discriminates `Inserted` vs `Duplicate`. Bump a per-URL
   `novel_events` (resp. `duplicate_events`) counter in the same match
   arms. The URL is in scope as `provenance: &str` at line 62. Don't
   reach for `ProvenanceEntry::primary` — the `Inserted` discriminant
   *is* the cleaner signal; `primary` is the store-side post-sort
   convention.

3. **Per-URL metrics state.** Add `relay_url_health: HashMap<String,
   RelayUrlMetrics>` to the `Kernel` struct (sibling of the existing
   role-keyed `relays: HashMap<RelayRole, RelayHealth>` at
   `kernel/mod.rs`). Every `RelayEvent` already carries `.relay_url()`
   (`relay_worker.rs`), so `dispatch::handle_relay_event` and the ingest
   path see the URL on every frame — the metrics writer is a few lines
   per arm. EOSE timing is `Instant::now().duration_since(opened_at)`
   from the existing `WireSub::opened_at`.

4. **Dispatch-drop counter projection.** T114 part 1 left
   `dispatch_drops: Arc<AtomicU64>` in `run_actor` scope
   (`actor/mod.rs:145`). The current `let _ = dispatch_drops` (line 150)
   keeps it alive but unread. The projection thread loads it into
   `RelayMetrics::dispatch_drops` once per emit. Per-URL split is
   deferred until T114 part 2 buckets the drops at try_send time;
   until then the total fans out by RR or by "biggest sender" — call
   the policy out explicitly when wired.

5. **Snapshot projection.** `kernel/status.rs::wire_subscriptions`
   (line 202) already filters `WireSub` for the projection — add a
   sibling `relay_diagnostics()` that walks `relay_url_health.values()`
   and folds in the URL-filtered `wire_subs` slice. The result becomes
   `KernelUpdate::relay_diagnostics`. The opt-in flag short-circuits to
   `None`.

No changes required in the worker — attribution is kernel-side state;
the worker still only knows `(role, url, tx, generation)`.

---

## 6 — Chirp UI sketch (read-only spec)

`ios/Chirp/` — out-of-kernel consumer; this section is a target
description, **no implementation touches the iOS tree from this doc**.

* **One row per URL** (single-socket invariant §1). Row header:
  `wss://relay.damus.io · CONNECTED · auth: authenticated · 12 subs`.
* **Expander 1 — "Why we're here":** chips per `Attribution`.
  e.g. `[NIP-65 write · 47 authors · alice, bob, +45]`,
  `[bootstrap-discovery]`, `[relay-pin · interest #34]`. Chip tap →
  modal listing the bounded `authors` slice + `source_event_ids` short
  hash.
* **Expander 2 — "What we're sending":** the URL-filtered `subs` list.
  Filter summary, state, EOSE elapsed, events delivered. Sub-tap reveals
  the `interest_id` (link back to the lattice debug view).
* **Expander 3 — "What we're getting":**
  – Big number: **novelty ratio** (novel / total) with sparkline.
  – Latency histogram (eight bars, log-scale).
  – Frames/events/EOSE/bytes counters.
  – Recent notices/errors (bounded last 32).
* **Sort default:** novelty-ratio descending (most useful first).
  Secondary: bytes-in (most expensive first) for cost analysis.

The screen is a SwiftUI `List`; no Chirp business logic touches the
kernel data model. Reuse the existing `KernelUpdate` ingestion path —
the new field is JSON-decoded into Codable structs and rendered.

---

## 7 — Follow-up tasks

Tnnn handles assigned by the orchestrator; ordering reflects dependency
chain (F1 → F2 → F3 → F4; F5 in parallel; F6 last as the consumer).

| Tag | Name                                  | Touches                                                                                  |
|-----|---------------------------------------|------------------------------------------------------------------------------------------|
| F1  | `attribution-thread-through-emit`     | `kernel/outbox.rs:37` callers; `kernel/requests/mod.rs:227`; new kernel field            |
| F2  | `first-source-counter-on-insert`      | `kernel/ingest/timeline.rs:68` match arms; per-URL metrics state                         |
| F3  | `relay-url-health-state`              | New `Kernel::relay_url_health`; wired from `actor/dispatch.rs::handle_relay_event`       |
| F4  | `diagnostic-projection-on-snapshot`   | `kernel/status.rs` new `relay_diagnostics()`; `KernelUpdate` opt-in field; T103 envelope |
| F5  | `relay-control-role-aggregate`        | `actor/mod.rs:130` `RelayControl::role` → `roles: SmallVec<Role>` *or* demoted-to-label  |
| F6  | `chirp-outbox-explorer-screen`        | `ios/Chirp/` (read-only consumer; out-of-kernel)                                         |

F5 is the coordinator's load-bearing follow-up: the URL is the pool key
(T105 enforces this), but the per-control `role` is still singular.
Either lift it to an aggregate (mirrors `why_connected` cardinality) or
demote to a diagnostic-only label (the URL-keyed `why_connected` set is
the source of truth). The latter is the cleaner D7 split.

---

## 8 — What this is NOT

* Not an FFI redesign — single new optional field on `KernelUpdate`.
* Not a CompiledPlan consumer — the planner's `RelayPlan.role_tags`
  /`originating_interests` (`planner/plan.rs:144`) are the shape the
  `Attribution` mirrors, but the runtime emit path doesn't consume
  `CompiledPlan` yet (G2 in the lifecycle research doc). Convergence is
  cheap when G2 lands; until then the kernel records attribution at the
  partition-emit seam directly.
* Not a policy engine — D7. The kernel **reports** novelty ratio; it
  does not auto-drop "useless" relays. That's a shell decision.
* Not a Chirp feature — D0. Chirp is the first consumer; the data model
  is generic kernel diagnostics.
