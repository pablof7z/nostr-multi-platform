---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: superseded
subjects:
  - rung6-feed-gating
  - m1-fingerprint-omit
  - feed-change-signal
supersedes: []
related_claims: []
source_lines:
  - 9898-10024
captured_at: 2026-06-14T12:34:35Z
---

# Episode: Feed gating: M1 encoded-payload fingerprint, not O(1) dirty counter (Rung 6)

## Prior State

The home feed projection had no change-signal; it re-serialized a byte-identical ~58.8 KB payload every idle 4Hz tick (~6× the rest of the frame). The feed is host-registered (Tier-1), so Rung 3's kernel-manifest rev-gating could not help it — the engine snapshots + encodes unconditionally every tick

## Trigger

Measurement spike on the real op_feed engine confirmed the idle waste (58.8 KB/tick, byte-identical across 40 idle ticks, ~129 µs release encode). The feed is the dominant remaining byte/bandwidth/battery cost. Release-vs-debug timing (129 µs vs 2,266 µs, 17.6× factor) showed the jank attribution is not yet proven for release builds

## Decision

Adopted M1 — fingerprint the encoded FlatBuffers payload bytes, omit on identical fingerprint via producer-closure Seam A (returning Ok(None)), stamping a monotonic per-epoch projection_rev on changed emission. The fingerprint is trap-proof by construction (no change-signal can be missed because the generation is a pure function of the exact bytes the host receives). The engine's snapshot() method stays untouched. Monotonic rev (not hash) used for host reorder guard

## Consequences

- Omit==retain at host layer confirmed (ProjectionCache) — a missed change-signal would permanently freeze the timeline, making trap-proofness the cardinal requirement that ruled out M2 (O(1) dirty counter)
- Feed reaches host through typed sidecar only; generic Value slot is off-wire, not a correctness surface
- ~129 µs release encode at 4Hz is negligible vs freeze risk, so M2/M3's CPU savings don't justify their enumeration completeness risk
- Cardinal-trap test design specified: Group A (must-emit on real change), Group B (false-resend tolerance), Group C (host-coherence sim across epochs)
- Option B (per-row feed deltas) stays gated behind a release/device measurement proving busy-feed byte waste

## Open Tail

- R6-S1 implementation (emission state + should_emit helper + cardinal-trap tests) ready to dispatch on greenlight
- Release/device jank measurement still needed to confirm whether feed encode is the actual jank bottleneck vs Debug build or SwiftUI re-render

## Evidence

- transcript lines 9898-10024

