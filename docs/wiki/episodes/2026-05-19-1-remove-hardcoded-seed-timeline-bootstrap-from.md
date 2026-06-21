---
type: episode-card
date: 2026-05-19
session: 5d180e52-7c43-4a99-bfc4-769eb40dc03f
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/5d180e52-7c43-4a99-bfc4-769eb40dc03f.jsonl
salience: product
status: active
subjects:
  - seed-timeline-removal
  - timeline-bootstrap
  - follow-feed
supersedes:
  - 2026-05-19-3-remove-hardcoded-seed-timeline-bootstrap-from
related_claims: []
source_lines:
  - 1-53
  - 657-665
  - 830-858
  - 1197-1198
  - 1229-1241
captured_at: 2026-06-18T04:26:26Z
---

# Episode: Remove hardcoded seed timeline, bootstrap from active account's follows

## Prior State

The kernel hardcoded three pubkeys (fiatjaf, jb55, pablof7z) as 'seed accounts' used as the bootstrap timeline at cold start. Even after the active user's kind:3 contacts arrived, sync_follow_feed_interests permanently unioned those three pubkeys back into timeline_authors, so every logged-in user always saw posts from the hardcoded trio in addition to their own follows. The UI status line read SeedTimeline(fiatjaf,jb55,pablof7z).

## Trigger

User reported that chirp showed SeedTimeline(fiatjaf,jb55,pablof7z) instead of the logged-in user's feed, and clarified the original design intent: apps should simply express 'authors:[current-users-follows]' and nmp-core should transparently resolve that via kind:3/10002 without the app knowing about the protocol mechanics.

## Decision

Removed the hardcoded seed timeline entirely. startup_requests now bootstraps from the active account's own kind:3, profile, and relay list (not the three hardcoded pubkeys). sync_follow_feed_interests no longer unions seed pubkeys into timeline_authors and instead adds the active user's own pubkey. should_open_timeline gates on the active account's contacts rather than all three seed contact lists. The SeedAccount struct, seed_accounts() function, and FIATJAF_PUBKEY/JB55_PUBKEY constants were deleted. sign_in_nsec, create_account, and switch_active now call reconcile_follow_feed_after_identity_change so the timeline retargets on every identity mutation.

## Consequences

- Timeline content is now exclusively the logged-in user's follows — no developer accounts injected
- M2 follow-feed interest path is the sole timeline bootstrap mechanism; the M1 seed-timeline-* REQ path is fully retired
- Identity mutations (sign-in, create, switch) immediately retarget the follow feed without waiting for a subsequent kind:3 arrival
- Status line reports 'Timeline' instead of 'SeedTimeline(fiatjaf,jb55,pablof7z)'
- Cold start with no active account falls back to TEST_PUBKEY for self-profile/relay-list lookups only (no seed content feed)
- Discriminating regression tests added: timeline_authors excludes hardcoded seeds after kind:3 arrival; empty-follows clears stale interests; account switch reconciles follow-feed to new account

## Open Tail

- The REPL's $follows variable substitution still exists only in nmp-repl, not in nmp-core's InterestShape — a future 'template authors' feature could make the follow-feed declarative at the subscription level rather than procedurally registered

## Evidence

- transcript lines 1-53
- transcript lines 657-665
- transcript lines 830-858
- transcript lines 1197-1198
- transcript lines 1229-1241

