---
title: Kernel Filter Semantics and None vs Empty
slug: kernel-filter-semantics
topic: kernel-boundary
summary: When parsing app-supplied filter JSON, `None` (no constraint) must not be collapsed into `Some(empty)` (matches nothing), and vice versa.  For `open_interest(fi
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-12
updated: 2026-06-12
verified: 2026-06-12
compiled-from: conversation
sources:
  - session:954c56b2-d292-4021-8b55-977d3fd8df4d
---

# Kernel Filter Semantics and None vs Empty

## Filter Semantics

When parsing app-supplied filter JSON, `None` (no constraint) must not be collapsed into `Some(empty)` (matches nothing), and vice versa. Filter set fields should use `Option<BTreeSet<T>>` to preserve the None-vs-empty distinction required by NIP-01 (no constraint vs matches nothing). The NMP M2 `open_interest` migration must also preserve this None-vs-Some(empty) distinction for filter set fields, not collapsing an empty array into unconstrained.

<!-- citations: [^954c5-4] [^954c5-19] -->

## Open Interest Migration

The `nmp_app_open_timeline` function hardcodes kinds {1,6} and is tracked debt to be replaced by `nmp_app_open_interest(filter_json)`. The user decided to fix `nmp_app_open_timeline` by folding it into the generic `nmp_app_open_interest` seam. (Previously: The function was tracked as debt to be replaced once the ADR is written and merged.) For the M2 `open_interest(filter_json)` migration, the kernel should either validate tag keys server-side with loud errors or ship a typed filter builder in the bindings, because raw tag-key typos (e.g. "t" vs "#t") fail silently. <!-- [^954c5-20] -->
