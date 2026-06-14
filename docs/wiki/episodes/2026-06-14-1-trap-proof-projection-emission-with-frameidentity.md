---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: superseded
subjects:
  - projection-emission
  - frame-identity
  - feed-emission-state
  - app-reset-freeze
supersedes:
  - 2026-06-14-1-feed-emission-freezes-on-reset-rebaseline
related_claims: []
source_lines:
  - 10236-10366
  - 10383-10543
captured_at: 2026-06-14T16:20:53Z
---

# Episode: Trap-proof projection emission with FrameIdentity reset signal

## Prior State

FeedEmissionState tracked byte-identical changes but had a freeze bug on app reset — its omit-memory (last_emitted bytes) lived outside the kernel and survived nmp_app_reset with stale data while the host ProjectionCache was cleared via removeAll(), producing frozen/blank timelines. Two other Tier-1 projections (claimed_event_embeds, nip46_onboarding) always emitted Changed, causing ~40% residual hash-waste. A bespoke emission_epoch mechanism partially handled account-switch but was separate from the session axis.

## Trigger

Opus review found that on nmp_app_reset, the kernel rebuilds (new session_id → new session_id → host ProjectionCache removeAll()), but FeedEmissionState's omit-memory survived with last_emitted=Some(pre-reset bytes) and the preserved engine held old roots → next tick produced identical bytes → omit → host had no cached feed → frozen/blank timeline. Determinism (Ruling #1) and flag-divergence (Ruling #2) both passed; the lifecycle axis was the trap.

## Decision

Key producer rebaseline on FrameIdentity(session_id, snapshot_epoch) — the exact two-axis signal the host resets on. publish_frame_identity runs at the top of make_update before any projection closure, guaranteeing fresh values each tick. Generalized FeedEmissionState into TypedProjectionEmissionState in nmp-core (nip01 keeps only a transparent type alias). Deleted the bespoke emission_epoch mechanism (subsumed by snapshot_epoch). Collapsed the duplicate incremental-apply flag to a single Arc<AtomicBool> source of truth. All three Tier-1 projections (feed, claimed_event_embeds, nip46_onboarding) now share one mechanism.

## Consequences

- Freeze-safe app-reset: session_id change forces full baseline emit in lockstep with host cache removeAll(), empirically proven by c10 test (fails pre-fix, passes post-fix)
- Account-switch subsumed: snapshot_epoch bump on account-switch forces rebaseline without bespoke emission_epoch
- ~97.6% idle frame-byte reduction validated at whole-product level (45,440 → 1,104 B)
- lib.rs reduced below baseline (2976→2933 LOC) — debt reduced rather than increased
- Poisoned-registry fallbacks fail safe to always-emit (full rows), never to frozen omit

## Open Tail

- Option B row-deltas deferred post-v1 — a mutating in-window event still re-sends the whole feed
- R6-S5 release/device jank measurement pending — the honest answer to whether felt jank is actually fixed

## Evidence

- transcript lines 10236-10366
- transcript lines 10383-10543

