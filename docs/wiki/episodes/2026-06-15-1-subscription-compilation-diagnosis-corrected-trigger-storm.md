---
type: episode-card
date: 2026-06-15
session: c9a794f6-6ad7-4ee9-a620-fc342fd495c3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/c9a794f6-6ad7-4ee9-a620-fc342fd495c3.jsonl
salience: root-cause
status: superseded
subjects:
  - nmp-core-subs-compiler
  - subscription-lifecycle
  - compile-trigger-storm
supersedes:
  - 2026-06-15-1-chirp-ios-performance-root-cause-unconditional
related_claims: []
source_lines:
  - 842-860
captured_at: 2026-06-15T08:00:37Z
---

# Episode: Subscription compilation diagnosis corrected: trigger storm, not unconditional recompile

## Prior State

Profile trace attributed 19.3% CPU to SubscriptionCompiler::compile_with_context recompiling all subscriptions from scratch on every actor drain tick; proposed fix was per-subscription compile memoization.

## Trigger

Opus architectural review read drain_tick source (recompile.rs:235) which early-returns on empty trigger inbox — compile only fires when a CompileTrigger is enqueued. 19.3% weight means something is spuriously enqueuing triggers on nearly every tick. Also found the compiler is whole-set (not per-subscription), so per-subscription caching cannot work.

## Decision

Root cause reframed: investigate the CompileTrigger producers (actor/dispatch.rs, kernel/ingest/) to find why triggers are enqueued every tick. Defensive layer is input-keyed plan memoization at the recompile_and_diff seam (hash: interest snapshot + mailbox generation + dead_relays + app_relays + watermark generation), not per-subscription caching.

## Consequences

- Memo key MUST include watermark store generation — omitting it serves a stale `since` and silently under-fetches
- Per-subscription memoization approach is dead — the compiler does cross-interest greedy merge, not per-subscription compilation
- Trigger storm investigation is prerequisite; memoization papers over the symptom if the real bug is in trigger coalescing/dedup

## Open Tail

- Which CompileTrigger variant fires on every tick (InvalidateCompile, Nip65Arrived, ViewOpened)?
- Is there a dedup/coalescing gap in the trigger inbox?

## Evidence

- transcript lines 842-860
