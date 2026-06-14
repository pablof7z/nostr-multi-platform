---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: root-cause
status: superseded
subjects:
  - feed-emission-omit
  - projection-emission-state
  - frame-identity
  - reset-freeze-bug
supersedes:
  - 2026-06-14-1-feed-projection-becomes-trap-proof-omitting
related_claims: []
source_lines:
  - 10014-10026
  - 10138-10233
  - 10249-10271
  - 10312-10358
  - 10362-10364
  - 10383-10486
  - 10498-10527
captured_at: 2026-06-14T15:01:33Z
---

# Episode: Trap-proof projection omit with Reset-safe FrameIdentity rebaseline

## Prior State

The home feed re-serialized ~58.8 KB of byte-identical payload every idle 4 Hz tick (the dominant remaining frame waste). Tier-1 whole-value keys (claimed_event_embeds, nip46_onboarding) always emitted Changed. Projection omit-memory lived in captured closures outside the kernel, keyed only on an account-switch epoch (emission_epoch) that was not the same signal the host ProjectionCache resets on.

## Trigger

Empirical measurement (R3-S5 capstone) identified 58.8 KB/tick as the biggest idle waste. Design analysis chose M1 (byte-fingerprint of encoded payload, omit on identical) over M2 (O(1) counter) and M3 (skip-gate) because M1 is trap-proof by construction — the change-signal is a pure function of the exact bytes the host receives, so a missed bump cannot exist. Adversarial Opus review then discovered a concrete frozen-feed-on-Reset path: FeedEmissionState survives ActorCommand::Reset (lives outside the kernel) while the host resets its cache on session_id — identical bytes → omit → blank/frozen timeline.

## Decision

Implemented exact byte-equality omit (memcmp of retained Vec<u8>, not a hash — zero collision risk on a surface where a collision = permanently frozen feed) for the feed and all Tier-1 projections. Producer rebaselines on FrameIdentity(session_id, snapshot_epoch) — the exact two-axis signal the host ProjectionCache resets on — eliminating the bespoke account-switch-only emission_epoch mechanism. FeedEmissionState generalized to TypedProjectionEmissionState in nmp-core (single shared implementation, no duplication). Capability-OFF path is byte-identical to pre-change behavior (always emits). Duplicate incremental_apply Arc<AtomicBool> flags collapsed to one source of truth in SnapshotRegistry.

## Consequences

- ~58.8 KB/tick eliminated on idle feed ticks; ~40% residual from always-Changed keys also eliminated
- Reset-freeze bug closed: FeedEmissionState surviving ActorCommand::Reset while host clears cache no longer causes frozen/blank timeline — producer and host now rebaseline on the identical (session_id, snapshot_epoch) tuple
- Bespoke emission_epoch + follow_set.on_change mechanism deleted (subsumed by snapshot_epoch)
- Engine (nmp-feed) untouched — all emission logic in producer-closure layer
- Per-key freeze guards (c10 session_id axis, c11 epoch axis) proven fail-before/pass-after
- publish_frame_identity runs at top of make_update before any projection closure, guaranteeing in-tick freshness; worst-case ordering errs toward emit (safe direction)
- lib.rs reduced below its baseline (2976→2933) via extraction, no baseline bumps

## Open Tail

- S4 capstone: register op_feed in ffi-stress, empirically prove idle feed bytes → ~0
- S5: release/device measurement — answers whether the original felt jank is actually fixed, and gates whether Option B row-deltas are worth pursuing
- nip46_onboarding has only generic freeze-test coverage (no dedicated per-key test file like claimed_event_embeds has) — symmetry nit

## Evidence

- transcript lines 10014-10026
- transcript lines 10138-10233
- transcript lines 10249-10271
- transcript lines 10312-10358
- transcript lines 10362-10364
- transcript lines 10383-10486
- transcript lines 10498-10527

