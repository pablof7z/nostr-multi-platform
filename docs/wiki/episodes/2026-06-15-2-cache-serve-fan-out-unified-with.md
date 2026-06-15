---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - cache-serve-fan-out
  - chokepoint-unification
  - profiles-ver-stale-projection
supersedes:
  - 2026-06-15-2-cache-serve-post-store-fan-out
related_claims: []
source_lines:
  - 3295-3314
  - 3607-3637
captured_at: 2026-06-15T15:36:07Z
---

# Episode: Cache-serve fan-out unified with live chokepoint via shared project_accepted_event helper

## Prior State

Cache-serve's feed_served_event systematically diverged from the live chokepoint's post-store fan-out — it called none of: D9 future-created_at clamp, mailbox/dm-relay/profile transition sweep, NIP-parser dispatch, or observer notify. Two independent bugs were found from the same root cause: D9 feed-warp (PR 1b) and stale profiles_ver after cold-restart cache-serve (PR 2 blocker #1).

## Trigger

Codex review of PR 2 found profiles_ver not bumped on cache-serve replay; combined with the already-identified D9 clamp gap in PR 1b, the pattern was clear: cache-serve is a systematic divergence, not isolated gaps.

## Decision

Extract one shared Kernel::project_accepted_event(verified) helper owning all three post-store concerns kind-agnostically (NIP-parser dispatch, transition sweep, D9 clamp + observer notify). Call it from both ingest_accepted_event (live) and feed_served_event (cache-serve). Fold PR 1b into PR 2 rework — abandon the narrow D9-clamp branch since both fixes live in the same file and the unified helper is the architecturally-right ADR-0045 'single mechanism' solution.

## Consequences

- PR 1b (narrow cache-serve D9 clamp) superseded — its logic ported into the shared helper
- Cold-restart cache-serve now correctly repopulates capability caches (profiles, mailbox, dm-relay) and bumps version counters
- Future-dated events from cache-serve are clamped in observer fan-out (store retains raw timestamp)
- Adding a new cache or transition concern requires updating only project_accepted_event — both paths inherit it automatically

## Open Tail

*(none)*

## Evidence

- transcript lines 3295-3314
- transcript lines 3607-3637
