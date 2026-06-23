# Intent-classed routing + NIP-50 search — design

> **Status:** ADR accepted; substrate/search pieces are partially implemented.
> **ADR:** `docs/decisions/0020-intent-classed-routing-and-search.md`.
> **Date:** 2026-05-18 (search reconciled higher-order, 2026-06-22).
> **Scope:** Core/planner own the generic search/index **seams** —
> `InterestShape.search` wire-filter field, generic blocked-relay subtraction,
> the noun-free `substrate::search` registry (`SearchScopeRegistrar` /
> `SearchScopeProvider` / `SearchScopeRegistry` / cache-serve hook), and the
> account-config self-kind bootstrap including kind:10007 — plus (for
> Draft/Wiki) the `EventClass` class-routing lane. ALL NIP-50 search
> orchestration (entrypoint, relay selection from kind:10007, cache scan,
> ranking, dedup, result projection) is higher-order in `nmp-nip50`. There is
> no `EventClass::Search`.

This document is split into focused sub-files to stay under the 500 LOC ceiling (`AGENTS.md`).

- [Goals + non-goals](intent-routing/goals.md) (§1, §2)
- [Type surface — `EventClass`, `InterestShape`, `OutboxResolver`, `PublishTarget`, search FFI, kernel-init defaults](intent-routing/types.md) (§3)
- [Planner integration — per-author partition, merge rule, blocked-relay post-filter, lazy 10102 fetch](intent-routing/planner.md) (§4)
- [NIP-51 fact stream — kind table, kind:30002 deferral, fact-stream wiring](intent-routing/nip51-facts.md) (§5)
- [Diagnostic discipline — seven-lane model + blocked subtractive filter](intent-routing/diagnostics.md) (§6)
- [Cache-side search — scan scopes, dedupe/merge, fanout policy](intent-routing/cache-search.md) (§7)
- [FFI ergonomics, test surface, future work](intent-routing/ffi-tests-future.md) (§9, §10, §11)

## Section map

| § | Topic | File |
|---|---|---|
| 1 | Goals (higher-order search; kind:10007 read by `SearchRelayListProjection`) | goals.md |
| 2 | Non-goals (no Lucene; DM/communities/named-sets deferred) | goals.md |
| 3 | Type surface: `EventClass` (no `Search` variant), `InterestShape.search`, `OutboxResolver`, `PublishTarget`, search FFI (lives in `nmp-nip50`), kernel-init defaults | types.md |
| 4 | Planner integration: per-author class-routing partition, Rule 10 search-equality merge, fail-loud blocked-relay post-filter, lazy 10102 lifecycle | planner.md |
| 5 | NIP-51 fact stream: kind→role table, kind:30002 deferral, `Nip51RoutingFacts` wiring (kind:10007 excluded — higher-order) | nip51-facts.md |
| 6 | Diagnostic discipline: seven routing lanes + blocked subtractive filter | diagnostics.md |
| 7 | Cache-side search: scan scopes, dedupe/merge, fanout policy (relay selection by `nmp-nip50`) | cache-search.md |
| 9 | FFI / app-developer ergonomics (Swift examples) | ffi-tests-future.md |
| 10 | Test surface (M-gate criteria; search tests live in `nmp-nip50`) | ffi-tests-future.md |
| 11 | Future work (incl. cache FTS inverted index — issue #1811) | ffi-tests-future.md |

(There is no §8 in this design; section numbers are preserved from the
original single-file document.)
