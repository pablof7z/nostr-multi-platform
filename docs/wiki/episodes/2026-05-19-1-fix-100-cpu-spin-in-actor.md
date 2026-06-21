---
type: episode-card
date: 2026-05-19
session: c4b2e655-ca6b-42d2-9383-89bf52215d0a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/c4b2e655-ca6b-42d2-9383-89bf52215d0a.jsonl
salience: root-cause
status: active
subjects:
  - nmp-core-actor-loop
  - kernel-emit-cycle
  - wire-subs-watermark
supersedes: []
related_claims: []
source_lines:
  - 10-10
  - 9960-9962
  - 10118-10128
  - 10130-10151
captured_at: 2026-06-18T04:33:25Z
---

# Episode: Fix 100% CPU spin in actor emit loop via watermark-based dirty gating

## Prior State

App pegged CPU at 100% when idle; the actor's tick/emit cycle was spinning continuously, re-emitting snapshot frames on every pass without checking whether state had actually changed.

## Trigger

User reported 'the app is not doing anything yet its using 100% of cpu in my iPhone'; debug eprintln added to emit_now confirmed continuous frame emission (NMP_EMIT emit_now running= frame_len=).

## Decision

Added watermark-based dirty tracking across the wire subscription, outbox, and store query subsystems so the actor only re-emits when state has materially changed; emit_now is no longer called unconditionally on every tick iteration.

## Consequences

- CPU usage drops to idle when no kernel state has changed
- Wire subscriptions track their own completion watermark to avoid re-polling already-satisfied queries
- Outbox tracks a dirty watermark so publish state changes are only emitted once
- App can now navigate past onboarding without pinning the CPU

## Open Tail

- Debug eprintln still present in tick.rs emit_now — must be removed before shipping
- Session was interrupted before final verification of CPU behavior on physical device

## Evidence

- transcript lines 10-10
- transcript lines 9960-9962
- transcript lines 10118-10128
- transcript lines 10130-10151

