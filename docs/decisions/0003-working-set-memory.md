# ADR 0003: Memory budget is for working set, not total cached events

**Date:** 2026-05-17
**Status:** accepted
**Supersedes:** `reactivity.md` rev 0 §10.3 memory budget

## Context

The initial gate read "≤ 100 MB at 100k events / 100 views." The reactivity-bench harness (run 001) reported 130.8 MB at 1M events, failing the gate. But this is misleading: holding 1M events resident in memory is the anti-pattern the spec already calls out for the durable storage backend (LMDB / SQLite / IndexedDB / nostrdb).

The actor should keep a **bounded working set** of hot events in memory; cold events live on disk. The reverse index can cover both — it keys on attributes, not event bodies.

## Decision

The memory budget targets **working-set memory at typical active load**, not total cached events.

| Metric | Budget |
|---|---|
| Working-set memory at 100 active views, 10k hot events | ≤ 100 MB |
| Total cached events on disk | unbounded (or capped by backend quota) |

Working-set policy:

- **Hot:** events referenced by any open view's claim set, plus a configurable recency window (default: most recent 5,000 events globally).
- **Cold:** everything else, on disk only.
- **Eviction:** LRU among hot events not currently claimed.

The reverse index indexes both hot and cold events. Lookup returns view ids immediately; event bodies for delta construction load lazily and synchronously via the storage backend.

Projection caches (`author_display`, `reaction_summary`, etc.) are LRU-bounded by referenced-view count; not every pubkey ever seen stays in the projection cache.

## Consequences

- The 1M-events-resident scenario is no longer a failure — it's an unintended test of an unintended configuration. Re-run with bounded working set.
- Cold-event delta construction has a one-time disk hit; this is acceptable for replaceable events (kind:0 re-load on profile fan-out) but worth measuring.
- Eviction policy needs explicit design; LRU is the default but priority-ordered (e.g., never evict claimed events) is the real invariant.

## Alternatives considered

- **Keep absolute gate, raise number.** Rejected — doesn't address the underlying anti-pattern.
- **Cap total cached events.** Rejected — the storage backend already handles this; the framework should not duplicate.
- **All-in-memory cache.** Rejected — doesn't scale and is contrary to the storage abstraction.

## Validation

Re-run reactivity-bench with bounded working set; require ≤ 100 MB at 100 views / 10k hot events / 1M cached events on disk.
