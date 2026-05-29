---
title: D5 Doctrine — Snapshot Keys Are Gated on Open Views
slug: d5-snapshot-key-gating
summary: Snapshot keys (e.g. projections.timeline) are only emitted when the corresponding view is open; shells and tests must call OpenContactListSubscription before expecting timeline keys.
tags:
  - d5
  - snapshot
  - doctrine
  - timeline
  - v46
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# D5 Doctrine — Snapshot Keys Are Gated on Open Views

> Snapshot keys (e.g. projections.timeline) are only emitted when the corresponding view is open; shells and tests must call OpenContactListSubscription before expecting timeline keys.

## Snapshot Keys Are Gated on Open Views (D5)

A snapshot key is only emitted when the corresponding view is open. For the timeline, `follow_feed_kinds` must be non-empty before `projections.timeline` appears in the snapshot.

`follow_feed_kinds` is set via `OpenContactListSubscription { kinds: {...} }`. Without this command, `projections.timeline` is absent from every snapshot even when events have been ingested. [^42908-43]

## V-46: Snapshot D5 Bounding

Previously, `crates/nmp-core/src/kernel/update/projections.rs` emitted snapshot keys unconditionally (20+ keys always present, regardless of whether any view was open). This violated D5 doctrine.

Fix (PR #770): the `projections.timeline` key (and related feed keys) are now gated on `follow_feed_kinds` being non-empty. Action keys are null-gated. The function is in `crates/nmp-core/src/kernel/update/projections.rs`. [^42908-44]

## Test Implication

Any test or shell that expects `projections.timeline` in the snapshot must send `SignInNsec` followed by `OpenContactListSubscription { kinds: {1} }` before ingesting events. The `c13` contract test was updated accordingly in PR #779. This mirrors what a real shell does. [^42908-45]

## See Also

