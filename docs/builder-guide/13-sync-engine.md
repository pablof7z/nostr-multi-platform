# 13 — Sync engine: `nmp-nip77` (NIP-77 first, REQ second)

> Status: **SHIPS** · Audience: **agents** · Doctrine: **D2** (negentropy first), D6, D8.

The sync engine is **not a feature** — it is planner *policy* over three
inputs: cache coverage, per-relay NIP-77 capability, and runtime progress
state. Live views always tail with REQ immediately; NIP-77 is
the preferred *historical backfill* mechanism, used only where support is
proven. This section is the agent's map of where each decision lives and
what you must not re-implement.

`nmp-nip77` is transport-agnostic at the core: the reconciler exchanges
opaque byte payloads; the WebSocket framing is a thin layer on top. Module
map is the table in `crates/nmp-nip77/src/lib.rs:11-21`.

## The reconciler (deterministic step API)

`Reconciler` (`crates/nmp-nip77/src/reconciler.rs:104-214`) wraps
`negentropy::Negentropy`. It is a pure state machine: feed it the peer's
bytes, it returns either `Send(bytes)` (forward to peer, call `step` again)
or `Done { have, need, state }`. `need` is what you REQ-fetch; `state` is an
opaque runtime resume blob for the NIP-77 session.

- **Client** drives (`Reconciler::client`, first `step(None)` produces the
  initial query). **Server** responds; `step(None)` on a server is a
  programmer error → `ReconcilerError::ServerNotInitiator`
  (`reconciler.rs:160-161`).
- Frame size is capped at `FRAME_SIZE_LIMIT = 64 KiB`
  (`lib.rs:78`) — D8 working-set bound. The `negentropy` 0.5 crate exposes
  no public deserializer, so `resume_client` (`reconciler.rs:146-151`)
  re-seeds from current items; the protocol still converges
  deterministically.

Wire framing (`NEG-OPEN` / `NEG-MSG` / `NEG-CLOSE` client→relay; `NEG-MSG` /
`NEG-ERR` relay→client) lives in `crates/nmp-nip77/src/wire.rs:28-49`. The
reconciler never sees JSON; `wire.rs` is the only module that knows about
WebSocket text frames.

## Triggers

Three triggers fan out into deduplicated `ReconcileWork`
(`crates/nmp-nip77/src/triggers.rs:31-46, 97-131`). The engine owns the
open-filter map; it never opens a socket or touches the store.

| Trigger | Fan-out | Source |
|---|---|---|
| `Foreground` | every open `(filter, relay)` pair | app returned to foreground |
| `ViewOpenedWithGap { filter_hash, relay_url }` | the single named pair | view opened **and** coverage was `PartialUpTo`/`Unknown` (never `CompleteAsOf`) |
| `RelayReconnected { relay_url }` | every `(filter, _)` on *that* relay only | WebSocket reopened / network resumed |

Output order is deterministic by the derived `Ord` on `ReconcileWork`
(filter-hash major, relay-url minor) — `triggers.rs:49-53`. An unknown relay
on reconnect produces an empty work list (silently, D6 — no error type).

## SyncStrategy decision matrix

The load-bearing decision is `decide_strategy`
(`crates/nmp-nip77/src/coverage_gate.rs:86-119`). It is **infallible** — no
`Result`, every input maps to one strategy (D6: the planner never surfaces a
"coverage decision failed" toast).

| `Coverage` | `supports_nip77` | resume state | → `SyncStrategy` |
|---|---|---|---|
| `CompleteAsOf(_)` | *any* | *any* | `SkipReq` — cache authoritative, no wire frame |
| `PartialUpTo(_)` / `Unknown` | `Some(true)` | none | `NegThenReq` |
| `PartialUpTo(_)` / `Unknown` | `Some(true)` | has runtime state | `Resume { next: NegThenReq, state }` |
| `PartialUpTo(_)` / `Unknown` | `Some(false)` | coverage floor `s` | `ReqSince(s + 1)` |
| `PartialUpTo(_)` / `Unknown` | `Some(false)` | none | `ReqSince(0)` |
| `PartialUpTo(_)` / `Unknown` | `None` (probe unrun) | — | conservative: treated as **not** NIP-77 → `ReqSince(...)` |

Notes that matter for agents:

- The cutover is **not** a numeric percentage check in the gate. The
  `COVERAGE_THRESHOLD_PCT = 95` constant (`coverage_gate.rs:34`) is
  documentation of the *store's* staleness policy; `decide_strategy` matches
  `Coverage::CompleteAsOf` directly because that variant *already* encodes
  the freshness window from `docs/design/lmdb/watermarks.md`. Do not
  re-derive freshness here.
- `freshness_ratio` (`coverage_gate.rs:145-156`) is a **recency** signal for
  diagnostics only — *not* a cache-completeness ratio. The planner gate
  never calls it.
- `+1` on `ReqSince` keeps the boundary event from being re-fetched.

### Applying the strategy to the plan

`apply_coverage_filter` (`crates/nmp-nip77/src/planner_gate.rs:70-115`)
rewrites a `CompiledPlan` in place per `(filter, relay)`:

| Strategy | Plan rewrite |
|---|---|
| `SkipReq` | sub-shape dropped; relay removed if its plan is now empty |
| `ReqSince(s)` | `shape.since` bumped to `s` (no-op if already ≥ `s`) **then `sub.recompute_hash()`** |
| `NegThenReq` | sub-shape kept unchanged; the negentropy run is on the parallel sync path |
| `Resume { next, .. }` | collapsed via `.inner()` to `next` |

The `recompute_hash()` after a `since` bump is **mandatory** — the
wire-emitter keys sub-ids by `canonical_filter_hash`, so without it the diff
sees no change and the new `since` never reaches the relay (M4 codex P1,
`planner_gate.rs:124-134`). This gate is installed into `nmp-core` via the
`PlanCoverageHook` seam — see [14 — Subscription lifecycle + relay manager + NIP-42](14-relay-manager.md).

## Runtime invocation

There is no app-facing `RunSync` action-machine surface in current
`nmp-nip77`. Sync is runtime policy installed behind the planner gate:
`NegentropySyncRuntime` (`crates/nmp-nip77/src/runtime.rs`) intercepts eligible
REQ frames, sends `NEG-OPEN`, processes `NEG-MSG` / `NEG-ERR` responses through
`Reconciler` (`reconciler.rs`), and falls back to ordinary REQ when the relay is
unsupported or silent past the liveness deadline.

Apps do not build per-relay sync actions. They open interests; the planner,
coverage gate, relay capability state, and runtime decide whether a historical
backfill uses NIP-77 or REQ.

## Capability probe state machine

A relay either speaks `NEG-OPEN` or it does not; discover once on first
contact, cache forever (`crates/nmp-nip77/src/capability.rs:36-54`):

```text
Unknown --begin()--> Probing --NEG-MSG--> Supported   (terminal)
                              --NEG-ERR--> Unsupported (terminal)
```

Relay support state is tracked in `runtime.rs` as `RelayNegentropyState`.
`Probing` starts when `NEG-OPEN` is sent; `NEG-MSG` marks the relay supported;
`NEG-ERR`, unsupported filters, or the liveness deadline mark it unsupported
for the session and drive the plain-REQ fallback.

## Metrics counters

Two monotonic `u64`s per `(filter_hash, relay_url)` pair
(`crates/nmp-nip77/src/metrics.rs:43-53`), keyed exactly like the watermark
table so diagnostics line up with the sync target:

| Counter | Increment trigger |
|---|---|
| `bytes_on_wire_via_neg` | every byte sent/received inside a `NEG-MSG` frame |
| `bytes_saved_vs_req` | `(REQ-baseline − negentropy-bytes)` for the pair, **clamped ≥ 0** |

`record_savings` clamps regressions to 0 (`metrics.rs:76-81`);
`MetricsSnapshot` is plain `serde` so the ADR-0007 diagnostics bridge ships
it through `AppState.debug` without a per-counter FFI wrapper. The sync
efficiency target is bytes-on-wire ≤ 5% of the equivalent REQ on a 10k-event
backfill.

## Anti-patterns

1. **Assuming every relay speaks NIP-77.** It is probed and cached per
   relay; `capabilities: None` is treated conservatively as *no* NIP-77, not
   "try anyway". Fan-out is per-relay — one relay can `NegThenReq` while
   another in the same fan `ReqSince`.
2. **Gating live reads on sync completion.** Live views tail with REQ
   *immediately* (D1/D2, `docs/product-spec/subsystems.md:242`). Sync
   backfills concurrently; never block a view payload on a reconciliation.
3. **Manual REQ scans in app code for backfill.** Backfill is planner
   policy. Hand-rolled "fetch last 30 days" loops bypass the watermark and
   the bytes-saved instrumentation. Use `RunSync`.
4. **Treating a watermark as "everything ever".** A watermark is "complete
   *as of* T", not "all events that exist". Reading it as the former is the
   over-fetch / authoritative-miss bug pair from
   `docs/design/framework-magic/sync.md` C10.
5. **Mutating `shape.since` without `recompute_hash()`.** The bump silently
   never reaches the wire (M4 codex P1). Only relevant if you write a custom
   coverage hook — use `apply_coverage_filter` and you get this for free.

## Deliverables recap

- **Triggers table** — three triggers, fan-out, source (above).
- **SyncStrategy decision matrix** — `Coverage × supports_nip77 × watermark`
  (above).
- **`RunSync` invocation example** — the Rust block above.
- **Metrics counters table** — the two-counter table (above).

See also: [07 — Subscription planner — Interest → CompiledPlan → wire](07-subscription-planner.md) · [08 — EventStore + insert invariants + GC](08-eventstore.md) · [14 — Subscription lifecycle + relay manager + NIP-42](14-relay-manager.md).
