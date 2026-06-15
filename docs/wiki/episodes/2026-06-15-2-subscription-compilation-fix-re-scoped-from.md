---
type: episode-card
date: 2026-06-15
session: c9a794f6-6ad7-4ee9-a620-fc342fd495c3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/c9a794f6-6ad7-4ee9-a620-fc342fd495c3.jsonl
salience: architecture
status: superseded
subjects:
  - subscription-compile-memoization
  - recompile-and-diff-short-circuit
  - push-interest-unconditional-trigger
supersedes:
  - 2026-06-15-1-subscription-compilation-diagnosis-corrected-trigger-storm
related_claims: []
source_lines:
  - 846-860
  - 1019-1074
captured_at: 2026-06-15T08:33:24Z
---

# Episode: Subscription compilation fix re-scoped from per-subscription cache to input-keyed plan memoization at lifecycle seam

## Prior State

Proposal 2 aimed to 'cache the compiled InterestShape lattice result per-subscription' inside recompile_and_diff, recompiling only when a subscription's input interest changed. The trace diagnosis stated 'recompiles all subscriptions from scratch on every actor drain tick.'

## Trigger

Opus review found the compiler is whole-set (compile_with_context takes &[LogicalInterest], partitions all authors across relays, does per-relay greedy merge) — there is no per-subscription compiled result to cache. Investigation found drain_tick already early-returns on empty inbox (recompile.rs:235); the 19.3% cost means triggers are being spuriously enqueued. Root cause: (1) push_interest_and_serve unconditionally enqueues InvalidateCompile with no shape comparison, and (2) recompile_and_diff has no input-equality short-circuit — every non-empty drain runs full O(authors×relays) compile even when inputs are byte-identical.

## Decision

Two-part fix: (A) Plan-input memoization at the recompile_and_diff seam — hash iter_active() shapes + mailbox-cache generation + dead_relays + app_relays + bootstrap sets + score-map generation + watermark store generation; if unchanged, return empty diff without invoking the compiler. (B) Gate push_interest_and_serve on shape change — have set_sub/push return whether the slot's interest actually changed, enqueue InvalidateCompile only on real change (mirroring ensure_interest_and_serve's if newly_installed gate).

## Consequences

- Memo key MUST include watermark store generation — omitting it serves a stale since timestamp and causes silent under-fetch
- current_plan (recompile.rs:134-137) exists but is only used for wire diff, not as a compile guard — it must be elevated to serve as the memo hit indicator
- Inbox-level dedup is the wrong layer — TriggerInbox is FIFO and coalescing only collapses within one tick; a single trigger per tick still forces full compile without the seam-level guard
- Fix A neutralizes every spurious trigger (including transient ViewOpened bursts) at one chokepoint; Fix B is cleanup of the lone unconditional producer

## Open Tail

- Need to confirm the exact memo key composition — especially whether the watermark rewrite (recompile.rs:316) mutates since independently of the interest set
- Follow-graph interest (follow_graph_interest) is re-pushed repeatedly during cold-start graph crawl as the WOT expands — the change gate in Fix B should suppress no-op re-pushes there

## Evidence

- transcript lines 846-860
- transcript lines 1019-1074
