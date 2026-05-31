---
title: NMP NIP-50 Search Capability
slug: nmp-nip50-search
summary: NMP supports a first-class search capability using NIP-50, where the `search` string field is passed in the filter and cache results are returned synchronously
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

# NMP NIP-50 Search Capability

## Search Capability

NMP supports a first-class search capability using NIP-50, where the `search` string field is passed in the filter and cache results are returned synchronously before relay results arrive. [^41858-9]


Search results are deduplicated by event_id with first-arrival wins semantics regardless of whether the source is the cache or a relay. [^41858-10]

## Search Scope

SearchScope defines the kind set for a search query: Users (kind 0), LongForm (kind 30023), an arbitrary set of kinds, or a custom InterestShape. [^41858-11]

## Search Targets

SearchTargets determines which relays to query: UserPreferred uses the user's NIP-51 kind:10007 list (falling back to an app-provided default), Explicit pins specific relays, and CacheOnly skips network requests. [^41858-12]

A search query sends the request to exactly one relay from the user's NIP-51 kind:10007 list per call to prevent aggregate intent leakage; apps needing broader coverage must use Explicit targets. [^41858-13]

## Relay Selection and Fanout

The planner applies blind fanout for relays in the user's kind:10007 search list, even if they do not support NIP-50, surfacing dead lanes via per-relay diagnostics rather than probing with NIP-11. [^41858-14]

## Cache Substrate

The cache substrate lazily builds a lightweight inverted index over kind:0 name/about/display_name and kind:30023 title/summary text only when a search is actually issued. [^41858-15]
## See Also

