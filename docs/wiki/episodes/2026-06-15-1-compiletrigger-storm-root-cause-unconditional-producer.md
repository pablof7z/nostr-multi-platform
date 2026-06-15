---
type: episode-card
date: 2026-06-15
session: c9a794f6-6ad7-4ee9-a620-fc342fd495c3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/c9a794f6-6ad7-4ee9-a620-fc342fd495c3.jsonl
salience: root-cause
status: active
subjects:
  - nmp-core-subs-recompile
  - compile-trigger-storm
  - push-interest-and-serve
supersedes:
  - 2026-06-15-2-subscription-compilation-fix-re-scoped-from
related_claims: []
source_lines:
  - 838-900
  - 1019-1075
captured_at: 2026-06-15T08:45:33Z
---

# Episode: CompileTrigger storm root cause: unconditional producer + no compile-input short-circuit

## Prior State

Trace diagnosed SubscriptionCompiler::compile_with_context (19.3% CPU) as recompiling all subscriptions from scratch on every actor drain tick; proposed fix was per-subscription InterestShape memoization.

## Trigger

Opus review read recompile.rs:235 and found drain_tick already early-returns on empty inbox — so triggers must be spuriously enqueued. Full audit of 13 CompileTrigger producers revealed one unconditional path.

## Decision

Two compounding root causes identified: (1) push_interest_and_serve (cache_serve/mod.rs:190-199) enqueues InvalidateCompile unconditionally — no shape comparison, unlike its correctly-gated sibling ensure_interest_and_serve; (2) recompile_and_diff (recompile.rs:56-71) has no input-equality short-circuit — every non-empty drain runs the full O(authors×relays) compile even when inputs are byte-identical. Fix A: plan-input memoization at the recompile_and_diff seam (hash iter_active() shapes + mailbox generation + dead_relays + app_relays + bootstrap sets + score-map generation). Fix B: gate push_interest_and_serve on shape change (have set_sub return whether interest actually changed).

## Consequences

- Memo key MUST include watermark store generation — omitting it produces stale-since under-fetch bug
- current_plan already exists at recompile.rs:134-137 but only used for wire diff, not as compile guard
- Per-subscription caching is architecturally wrong — the compiler is whole-set, not per-subscription; merge is a cross-interest product
- The 12 other trigger producers are correctly change-gated — not the source of the storm
- Inbox-level dedup is the wrong layer — TriggerInbox is FIFO, coalescing only collapses within one tick

## Open Tail

- Fix A + Fix B implementation pending user approval to start agent
- Need to confirm the memo hash covers all inputs read by compile_with_context + apply_selection + coverage_hook + watermark_rewrite

## Evidence

- transcript lines 838-900
- transcript lines 1019-1075
