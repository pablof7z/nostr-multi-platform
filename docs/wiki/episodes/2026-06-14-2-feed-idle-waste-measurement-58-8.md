---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: root-cause
status: superseded
subjects:
  - feed-idle-waste
  - adr-0055-rung6-trigger
supersedes:
  - 2026-06-14-3-feed-change-signal-mechanism-m1-fingerprint
  - 2026-06-14-4-feed-gating-m1-encoded-payload-fingerprint
related_claims: []
source_lines:
  - 9898-9947
captured_at: 2026-06-14T13:12:38Z
---

# Episode: Feed idle-waste measurement — ~58.8 KB byte-identical payload re-serialized every idle tick

## Prior State

Rung 3's 18%/68.8% result was assumed to be the primary byte win for the incremental emission ladder. The feed's contribution to frame waste was unmeasured — the bare S6 harness does not register the feed, so its numbers excluded the dominant projection.

## Trigger

Opus throwaway measurement spike on the real op_feed engine: the home feed re-serializes a byte-identical ~58.8 KB payload every idle 4 Hz tick (confirmed byte-identical across 40 idle ticks). The feed payload is ~6× the entire rest of the frame. The engine (op_feed_defaults.rs:267) snapshots + encodes unconditionally every tick with no rev/dirty/generation counter.

## Decision

The feed's idle re-serialization is the dominant uncovered byte/bandwidth/battery waste. Rung 6 (Tier-1/feed gating) is within the original O(changed-projections) mandate, not scope creep. Deferring it would be the exact comfort-deferral the user forbade. Option A (rev-gate the feed) proceeds; Option B (row-deltas) is gated behind release/device measurement.

## Consequences

- Rung 3's 18% byte win understated the whole-product picture — the feed alone is 6× the rest of the frame
- Release encode is 129µs vs debug 2,266µs (17.6× factor) — whether feed-gating fixes the felt jank is unproven for release builds, but the byte/bandwidth/battery waste is unambiguous
- Feed gating must handle the cardinal trap: omit==retain host-side, so a missed change-signal = permanently frozen timeline

## Open Tail

- Release/device jank measurement needed to confirm feed encode is the jank bottleneck (vs Debug-build overhead or SwiftUI re-render cost)
- If jank is not feed-encode-attributed, the remaining fix may be elsewhere (Debug build, SwiftUI) not in the emission ladder

## Evidence

- transcript lines 9898-9947

