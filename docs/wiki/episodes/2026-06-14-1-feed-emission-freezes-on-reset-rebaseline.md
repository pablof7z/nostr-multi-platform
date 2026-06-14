---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: root-cause
status: superseded
subjects:
  - feed-emission
  - frame-identity
  - projection-cache
  - reset-lifecycle
supersedes:
  - 2026-06-14-1-trap-proof-projection-omit-with-reset
related_claims: []
source_lines:
  - 10189-10202
  - 10237-10245
  - 10254-10257
  - 10312-10319
  - 10362-10366
  - 10576-10591
  - 10649-10659
captured_at: 2026-06-14T15:43:11Z
---

# Episode: Feed emission freezes on Reset — rebaseline keyed on host's actual reset signal

## Prior State

FeedEmissionState was 'trap-proof by construction' for content-byte changes but blind to the session/lifecycle axis. It lived outside the kernel, so on ActorCommand::Reset it survived with last_emitted=Some(pre-reset bytes) while the host ProjectionCache did removeAll() on session_id change — producing byte-identical payloads that were omitted into a now-empty host cache, yielding a blank/frozen timeline. The HostCacheSim only modeled the epoch axis, not session_id, making it a strawman on the Reset path.

## Trigger

Opus review (R6-S1) traced the Reset lifecycle: kernel rebuilds → new started_unix_ms → new session_id → host removeAll() → but FeedEmissionState (outside the kernel) retains last_emitted from pre-Reset → should_emit returns None (omit) → host has no cached feed. Tier-2 built-ins were immune because their omit-memory lives inside the kernel and dies on rebuild.

## Decision

Key producer rebaseline on FrameIdentity(session_id, snapshot_epoch) — the exact two-axis signal the host ProjectionCache resets on. Kernel publishes the tuple at the top of make_update (before any projection closure runs). The bespoke emission_epoch + follow_set.on_change mechanism was deleted (subsumed by snapshot_epoch, which already bumps on account-switch). FeedEmissionState generalized to TypedProjectionEmissionState shared by all three Tier-1 projections (feed, claimed_event_embeds, nip46_onboarding). Duplicate incremental_apply flags collapsed to a single Arc<AtomicBool>.

## Consequences

- Feed is reset-safe: c10 test proves FAIL-on-old logic, PASS-with-fix
- All three Tier-1 projections now omit when byte-identical, rebaselining in lockstep with the host cache
- 97.6% idle frame-byte reduction validated (45,440 → 1,104 B; feed 41,112 → 0 omitted on all idle ticks)
- R6-S1 FeedEmissionState became a thin re-export of TypedProjectionEmissionState in nmp-core; S2 generalized the mechanism without duplication
- S4 capstone false-resend gate tests the trivial case (stranger/out-of-follow-set) rather than the real over-invalidation risk (followed author, out-of-window event) — methodology gap noted but not yet fixed
- MiniProjectionCache models only the steady-state Changed/Cleared/retain subset, not session/epoch rebaseline — the S4 scenario never exercises that path

## Open Tail

- S4 false-resend probe should add a followed-author out-of-window event (older than 80th card) and assert no re-emit
- Optional actor-level integration test driving real ActorCommand::Reset through FFI to complement unit-level HostCacheSim proof
- R6-S5 (release/device jank measurement) still needed to answer whether the original felt jank is actually fixed

## Evidence

- transcript lines 10189-10202
- transcript lines 10237-10245
- transcript lines 10254-10257
- transcript lines 10312-10319
- transcript lines 10362-10366
- transcript lines 10576-10591
- transcript lines 10649-10659

