# Intent-classed routing + NIP-50 search §1–§2 — Goals and Non-goals

> Parent: `docs/design/intent-routing.md`.
> ADR: `docs/decisions/0071-write-intents-and-route-provenance.md`.

## 1. Goals

1. Apps call one function to search users / long-form / arbitrary kinds,
   get cache hits synchronously, and stream relay hits as they arrive.
   Search orchestration (entrypoint, relay selection, cache scan, ranking,
   dedup, result projection) lives entirely in `nmp-nip50` and above — this is
   the **higher-order search model** (ADR-0071 amendment 2026-06-22).
2. The kernel knows which relay class an event belongs to and routes
   accordingly, without app code naming relay URLs.
3. NIP-51 lists with routing semantics become live routing inputs,
   observed and applied the same way kind:10002 is today:
   - **10006** blocked relays (global filter via `blocked_relays()`).
   - **10007** search relays → read by `SearchRelayListProjection` in
     `nmp-nip51`; consumed by `nmp-nip50` directly (not a core routing class).
   - Future draft relay support must be introduced by the draft producer owner
     as an owner-declared `EventClass::Other(...)` value.
   - Future wiki relay support must be introduced by the wiki producer owner
     as an owner-declared `EventClass::Other(...)` value.
4. The diagnostic UI sees every class-routed decision as a distinct
   lane — no silent "the kernel just did something."
5. Raw user input resolves through one framework seam before free text reaches
   scoped search.

## 2. Non-goals

- A general full-text search engine. Cache-side search is opportunistic
  string scanning, not Lucene.
- DM routing (NIP-17 / kind:10050). Defer to its own ADR; this design
  reserves the `EventClass::DM` variant and decodes kind:10050 into the
  fact stream without consuming it.
- NIP-72 communities, NIP-90 DVMs, kind:30002 named relay sets — all
  default to `EventClass::Other` / NIP-65 routing in v1.
- Good wiki authors (kind:10101) — content allowlist, not relay routing.
- A product omnibox ranking policy.
