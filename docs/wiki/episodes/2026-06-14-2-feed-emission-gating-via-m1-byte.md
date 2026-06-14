---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: superseded
subjects:
  - adr-0055-rung6
  - op-feed-emit
  - frame-identity
  - projection-cache
supersedes: []
related_claims: []
source_lines:
  - 9903-9912
  - 10000-10025
  - 10146-10201
  - 10249-10287
  - 10312-10366
captured_at: 2026-06-14T14:10:09Z
---

# Episode: Feed emission gating via M1 byte-fingerprint with FrameIdentity reset safety

## Prior State

The op_feed engine had no change-signal or dirty counter; it unconditionally snapshot+encoded a ~58.8 KB payload every 4 Hz idle tick, even when byte-identical across 40 idle ticks. The feed was the only projection whose omit-memory would survive a kernel Reset (it lives outside the kernel in the producer closure)

## Trigger

Empirical measurement showed feed is ~6× the rest of the frame and re-serializes identically every idle tick; codex review confirmed M1 (fingerprint encoded bytes) is trap-proof by construction vs M2 (O(1) counter, enumeration risk) and M3 (skip-gate, same risk). Subsequent adversarial Opus review found Reset freeze: FeedEmissionState survives ActorCommand::Reset while host ProjectionCache clears on new session_id → omit into empty cache → blank timeline

## Decision

Adopted M1 exact-byte-equality fingerprint (memcmp, not hash — zero collision risk) in producer-closure Seam A. Producer rebaselines on FrameIdentity(session_id, snapshot_epoch) — the exact two-axis signal the host ProjectionCache resets on — eliminating the Reset freeze. Bespoke account-switch emission_epoch mechanism deleted (subsumed by snapshot_epoch). Duplicate incremental_apply flag collapsed to single Arc<AtomicBool> in SnapshotRegistry. Engine snapshot() untouched

## Consequences

- ~58.8 KB idle feed waste eliminated: unchanged feed omitted, host retains via ProjectionCache
- Reset freeze closed: FrameIdentity key forces baseline when either session_id or snapshot_epoch changes, matching host behavior
- emission_epoch + follow_set.on_change account-switch path deleted — subsumed by snapshot_epoch bump
- Single Arc<AtomicBool> source of truth for incremental_apply capability — eliminates two-flags-for-one-fact divergence
- lib.rs reduced from 2976→2933 LOC (below baseline) via incremental_apply.rs extraction
- 27 cardinal-trap tests including freeze guard c10 (proven fail-pre-fix/pass-post-fix)
- Capability-OFF path byte-identical to pre-change behavior (always emit)
- publish_frame_identity runs first in make_update, guaranteeing closure reads current tick values

## Open Tail

- R6-S2: gate the two small always-Changed Tier-2 keys (claimed_event_embeds, nip46_onboarding)
- R6-S4 capstone: register op_feed in ffi-stress harness, prove idle feed bytes → ~0
- R6-S5 release/device measurement: confirm whether remaining jank is feed encode, debug build, or SwiftUI re-render
- Option B (feed row-deltas) deferred behind empirical evidence of busy-feed serialization cost

## Evidence

- transcript lines 9903-9912
- transcript lines 10000-10025
- transcript lines 10146-10201
- transcript lines 10249-10287
- transcript lines 10312-10366

