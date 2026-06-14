---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: superseded
subjects:
  - feed-projection-omit
  - adr-0055-rung-6
  - frame-identity-reset-safety
supersedes:
  - 2026-06-14-2-feed-emission-gating-via-m1-byte
related_claims: []
source_lines:
  - 9893-9948
  - 9949-9989
  - 9993-10025
  - 10046-10080
  - 10138-10233
  - 10249-10286
  - 10312-10366
captured_at: 2026-06-14T14:18:03Z
---

# Episode: Feed projection becomes trap-proof omitting projection via byte-fingerprint + FrameIdentity

## Prior State

The home feed engine re-serialized a byte-identical ~58.8 KB FlatBuffers payload on every idle 4 Hz tick (~6× the rest of the frame), with no change-signal mechanism. The engine had no notion of 'did anything change' — it sorted all roots and cloned every visible card from scratch each tick. The feed was the only Tier-1 projection not rev-gated under ADR-0055.

## Trigger

Empirical measurement confirmed the feed emits byte-identical 58.8 KB payload across 40 idle ticks; user reaffirmed mandate to drive proper architecture with no technical debt and no comfort-deferral ('if the decision is clear, or can be clear by measuring, we need to try to make the best decision possible').

## Decision

Adopted M1 mechanism: exact byte-equality fingerprint (full memcmp, not hash) of the encoded FlatBuffers payload in the producer closure, omit on identical bytes, monotonic per-epoch rev on changed emission. Producer rebaselines on FrameIdentity(session_id, snapshot_epoch) — the same two-axis signal the host ProjectionCache resets on — rather than an account-switch-only internal epoch. The engine snapshot() is untouched; the emission decision lives entirely in the closure layer. Single Arc<AtomicBool> source of truth for the incremental_apply capability gate.

## Consequences

- ~58.8 KB/tick idle waste eliminated (FFI transfer + host decode + re-render eliminated on idle ticks); release encode cost (~129 µs/tick) remains
- Feed is now a properly omitting projection, but via a fundamentally different mechanism than Tier-2 built-ins (producer-closure fingerprint vs kernel-manifest gating)
- Host-registered projections cannot use kernel-manifest gating — their state lives outside the kernel; omit must happen in the producer closure (Ok(None) seam)
- Reset-freeze bug discovered in adversarial review and closed: FeedEmissionState survived ActorCommand::Reset while host cache reset on session_id change, which would have produced a blank/frozen timeline; fixed by keying on FrameIdentity(session_id, snapshot_epoch)
- Bespoke emission_epoch + follow_set.on_change mechanism deleted (subsumed by snapshot_epoch which already bumps on account-switch)
- incremental_apply flag collapsed from two sources (NmpApp mirror + SnapshotRegistry) to one Arc<AtomicBool>
- lib.rs reduced below its baseline (2976→2933) via extraction to incremental_apply.rs
- Feed encode is fully deterministic (BTreeMap, IndexMap/BoundedMessageMap, no relative-time fields), so byte-fingerprint omission fires reliably on idle

## Open Tail

- R6-S2 (gate two small always-Changed whole-value keys: claimed_event_embeds, nip46_onboarding) in progress
- Option B (feed row-deltas for per-row mutation efficiency) gated behind release/device measurement
- load_older/pagination not yet in the typed sidecar path (typed closure hardcodes FeedRequest::default())
- Actor-level integration test for Reset-through-FFI path not yet written (unit-level HostCacheSim proof exists)
- Release/device jank measurement (R6-S5) still needed to confirm whether the felt jank was feed-encode or SwiftUI/Debug-build

## Evidence

- transcript lines 9893-9948
- transcript lines 9949-9989
- transcript lines 9993-10025
- transcript lines 10046-10080
- transcript lines 10138-10233
- transcript lines 10249-10286
- transcript lines 10312-10366

