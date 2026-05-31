---
title: V-52 — Single-Relay Browsing (HIGH · v1 DX)
slug: v-52-single-relay-browsing
summary: "V-52 single-relay browsing (HIGH v1-DX): relay_pin reuse, zero actor/mod.rs changes, honest LMDB stub, delivered as PR #836."
tags:
  - backlog
  - V-52
  - relay
  - browsing
  - v1-dx
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# V-52 — Single-Relay Browsing (HIGH · v1 DX)

> V-52 single-relay browsing (HIGH v1-DX): relay_pin reuse, zero actor/mod.rs changes, honest LMDB stub, delivered as PR #836.

## Overview

V-52 is a HIGH-priority v1-DX backlog item: implement single-relay browsing capability. The feature allows a user to browse the timeline of a single relay rather than the full NIP-65 fan-out. The agent discovered that InterestShape::relay_pin plus the planner's case_e_relay_pinned already provide the 'one relay, no NIP-65 fan-out' invariant, so no redundant scope_relays field was needed — avoiding fragmentation per the project's anti-fragmentation rule. Only the missing store reverse-index was added, exposed via the nmp.browse_relay ActionModule with zero actor/mod.rs changes. [^4edd4-141]

## Review Findings

The Sonnet reviewer approved pending cargo test. All structural checks passed: zero actor/mod.rs changes confirmed at byte-level, relay_pin reuse is the correct ADR-0012 facility, store index maintained at all removal sites without drift, and LMDB receives an honest NotSupported stub rather than the V-17 Vec::new() anti-pattern. Two non-blocking fragilities were flagged as follow-ups: O(N-relays) removal scan, and exact-string URL matching (a subtle V-17-adjacent risk worth a future CanonicalRelayUrl normalization). [^4edd4-142]

## Merge

V-52 landed as PR #836 (merged at master 8211c189), the first correctly-prioritized HIGH item in the course-corrected wave. [^4edd4-143]

## Exoneration from Embed-Regression Suspicion

A peer agent suspected V-52 (#836) as the root cause of an embed-loading regression. Investigation proved V-52's mem/insert.rs change is purely additive and mechanical — it only adds relay_index_add/relay_index_remove calls alongside existing provenance logic, with a borrow-checker refactor (sources_after block) that is behaviorally identical. The actual event storage (st.events.insert), the claimed_events projection, and the claim_event request path are all untouched — none of them read relay_index. The peer later found the real cause: showcase events missing from seeded relays (purplepag.es + primal.net), only on nos.lol — a data-reachability issue, not a code bug. Exoneration evidence was posted on the PR itself to prevent the peer from reverting V-52 on a wrong bisect hypothesis. [^4edd4-144]

## See Also

