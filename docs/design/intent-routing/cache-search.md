# Intent-classed routing + NIP-50 search §7 — Cache-side search

> Parent: `docs/design/intent-routing.md`.
> Cross-refs: type surface (`SearchScope`/`SearchTargets`) in `types.md` (§3.5);
> future inverted-index work (#1811) in `ffi-tests-future.md` (§11).

## 7. Cache-side search

### 7.1 Scan scopes

- **`SearchScope::Users` (kind:0).** Linear over the kind:0 substrate
  slice. Profile cache is small (≤ ~10k entries in practice). Match
  against lowercased `name`, `display_name`, `about`, `nip05`. Substring
  match only — no fuzzy, no stemming.
- **`SearchScope::LongForm` (kind:30023).** Linear over the long-form
  substrate slice, scanning `title` tag, `summary` tag, and body prefix
  (capped at the first 4 KB of `.content`).

Both run synchronously inside `open_search` before returning the view.
**Migration baseline budgets:** ≤ 5 ms for 10k profiles, ≤ 20 ms for 1k
articles. Above those corpus sizes the linear scan becomes the bottleneck.
Scalable cache-side search is delivered by issue #1811 (cache FTS inverted
index epic): a visitor-style `text_search_visit` store seam + LMDB FTS
sub-databases + a crate-registered scope registry that lets `nmp-nip50` and
future protocol crates register their own FTS scope without touching core.
Until #1811 lands, linear scan is the only cache-side path; search-bearing
shapes that exceed the budget are relay-served only.

### 7.2 Dedupe and merge

Dedupe key is `event_id`. **First arrival wins**, whether the path is
cache or any of the N fan-out relays. Each `SearchHit` records a single
`source: SearchHitSource` — the first path that delivered the event.
Duplicate arrivals are silently dropped.

Ordering in the view:

- `cache_hits` is populated synchronously, sorted by relevance heuristic
  (substring start position, then `created_at` desc). This is what the
  app renders before any relay responds.
- `relay_hits` is appended in arrival order, deduplicated against
  `cache_hits` and against itself. Apps may resort client-side; the
  kernel provides arrival order as the canonical stream.

### 7.3 Fanout policy

`SearchTargets::UserPreferred` fans REQ out to **all** relays in the
user's kind:10007 list — no cap. Relay selection is performed by `nmp-nip50`
reading `SearchRelayListProjection` from `nmp-nip51`; this is NOT routed
through the core planner's class-routing machinery. No NIP-11
`supported_nips` probing; relays that don't implement NIP-50 surface as
zero-result lanes in the per-relay diagnostic. If kind:10007 is
missing/empty, fall back to `DefaultRelayLists::search`; if that's also
empty, only cache results are returned. Blocked-relay subtraction is applied
by `nmp-nip50` using `blocked_relays()` from the resolver. Until cache
indexing lands (#1811), search-bearing shapes are relay-served only and
deliberately uncovered by cache-serve.
