---
type: episode-card
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: root-cause
status: active
subjects:
  - nmp-core
  - nmp-nip01
  - crate-boundaries
  - timelineitem
supersedes:
  - 2026-06-13-3-timelineitem-naive-move-would-create-crate
related_claims: []
source_lines:
  - 8140-8144
  - 8160-8176
captured_at: 2026-06-13T20:04:54Z
---

# Episode: #920: naive TimelineItem move creates crate cycle — envelope-cut is the correct fix

## Prior State

#920 appeared to be a straightforward 'move TimelineItem out of nmp-core into nmp-nip01' fix for the D0 thin-shell violation

## Trigger

Opus agent verified the live dependency graph and found that nmp-nip01 → nmp-core already exists (Cargo.toml:11), making the naive move create a cycle

## Decision

The correct fix is the snapshot-envelope cut (nmp-nip01 already owns ModularTimelineSnapshot/TimelineEventCard), not a naive move of TimelineItem

## Consequences

- Naive 'move TimelineItem' approach is permanently ruled out — it would create a circular dependency
- #1283 and #920 share the same architectural pattern: resolve protocol-specific branching one layer above the kernel, ship typed, kernel stays D0-clean
- #920 reclassified as status:staged for post-v1 with a phased migration plan

## Open Tail

*(none)*

## Evidence

- transcript lines 8140-8144
- transcript lines 8160-8176

