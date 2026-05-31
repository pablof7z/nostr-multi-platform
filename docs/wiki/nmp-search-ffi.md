---
title: NMP Search — FFI Scope, Targets, and Tailing View
slug: nmp-search-ffi
summary: NMP exposes a first-class search capability via FFI with SearchScope (Users, LongForm, Kinds, Custom) and SearchTargets (UserPreferred, Explicit, CacheOnly) enu
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-18
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:41858cd2-3a5d-4ad1-bdd0-4cbe1df2dd9d
---

# NMP Search — FFI Scope, Targets, and Tailing View

## Search FFI Interface

NMP exposes a first-class search capability via FFI with SearchScope (Users, LongForm, Kinds, Custom) and SearchTargets (UserPreferred, Explicit, CacheOnly) enums. [^41858-26]


Search queries return a standard tailing view: cache hits emitted synchronously, then relay results merged as they arrive. [^41858-27]

The cache substrate builds a lightweight inverted index over kind:0 name/about/display_name and kind:30023 title/summary lazily (only when a search is actually issued), not eagerly. [^41858-28]

Search fanout uses blind fanout to relays in the user's kind:10007 list; non-NIP-50 relays produce zero-result lanes surfaced in per-relay diagnostics. [^41858-29]

Search result deduplication uses event_id as the dedupe key, with first-arrival winning regardless of source (cache or relay). [^41858-30]
## See Also

