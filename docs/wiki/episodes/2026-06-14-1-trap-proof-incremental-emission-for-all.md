---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: active
subjects:
  - projection-emission
  - frame-identity
  - feed-omit
  - freeze-guard
supersedes:
  - 2026-06-14-1-trap-proof-projection-emission-with-frameidentity
related_claims: []
source_lines:
  - 10323-10358
  - 10599-10609
captured_at: 2026-06-14T17:21:04Z
---

# Episode: Trap-proof incremental emission for all Tier-1 projections

## Prior State

All Tier-1 projections (feed, claimed_event_embeds, nip46_onboarding) were always-Changed, emitting full payloads every tick even when byte-identical (~41KB feed per tick on idle). Emission state was feed-specific (FeedEmissionState). A frozen-feed-on-Reset bug existed: the producer omitted unchanged frames into a host ProjectionCache that Reset had just cleared, leaving a blank timeline.

## Trigger

ADR-0055 Rung 6 to eliminate the dominant 58.8KB/tick idle waste. Opus review caught the freeze-on-Reset bug: epoch-only rebaseline key meant a session_id change (account switch or Reset) produced byte-identical frames that were omitted into a cleared cache. Empirically falsified: test c10 fails pre-fix, passes with the FrameIdentity fix.

## Decision

All three Tier-1 projections now omit when byte-identical, keyed on FrameIdentity(session_id, snapshot_epoch) — the same two-axis signal the host ProjectionCache resets on. FeedEmissionState generalized into a shared TypedProjectionEmissionState in nmp-core (nip01 keeps only a transparent type alias). Reset survival proven: take/set_snapshot_projection_handle_for_reset moves the same Arc holding frame-identity atomics onto the rebuilt kernel, so the surviving closure and new kernel share atomics. publish_frame_identity runs before run_typed_projections within make_update, guaranteeing fresh reads.

## Consequences

- 97.6% idle frame-byte reduction (45,440B → 1,104B per tick; feed payload 41,112B → 0 omitted on 8/8 idle ticks)
- Freeze guard per-key tested: session_id change + identical bytes forces baseline; epoch change + identical bytes forces baseline
- Emission state pattern is now shared/reusable — adding new Tier-1 keys requires only wrapping with Arc<Mutex<TypedProjectionEmissionState>>
- Capability-OFF path is byte-identical to today (always-emit); poisoned-registry fallbacks fail safe to full rows
- Nightly CI gate (ffi-stress feed-idle --fail-on-gate) guards the idle-omission invariant

## Open Tail

- Host-side TypedHomeFeedDecoder.decode still runs every tick on the retained feed regardless of changedKeys — gated by D5 bounded-native-state doctrine tension
- On a mutating frame the full feed still re-sends (row-deltas deferred)

## Evidence

- transcript lines 10323-10358
- transcript lines 10599-10609

