---
type: episode-card
date: 2026-05-18
session: fc128f85-af57-41cd-8c5b-a71d15450e17
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/fc128f85-af57-41cd-8c5b-a71d15450e17.jsonl
salience: product
status: superseded
subjects:
  - seed-timeline
  - startup-requests
  - timeline-ingest
supersedes: []
related_claims: []
source_lines:
  - 45-45
  - 316-332
  - 334-334
  - 788-825
  - 1017-1025
  - 1094-1100
captured_at: 2026-06-18T04:16:31Z
---

# Episode: Remove seed timeline; show only logged-in user's follows

## Prior State

The kernel hardcoded three seed accounts (fiatjaf, jb55, pablof7z) to populate a 'seed timeline' on cold start, providing content before the user signs in. `startup_requests()` emitted REQs for these seed accounts' profiles and notes; `maybe_open_timeline()` built its author set from `seed_accounts()`; the subscription id prefix was `seed-timeline-`.

## Trigger

User correction at line 334: 'remove the seed timeline, it's supposed to just show the logged in users' feed!' — overriding the prior assumption that a cold-start timeline needed pre-seeded content.

## Decision

Removed all seed timeline machinery: `SeedAccount` struct, `seed_accounts()` function, `FIATJAF_PUBKEY`/`JB55_PUBKEY` constants. `startup_requests` now fetches only the active account's self profile, self relay list, and self kind:3 contacts (returns empty if not signed in). `maybe_open_timeline` builds the author set solely from the logged-in user's contacts; `should_open_timeline` gates on the active account's kind:3 arrival or a 3-second deadline. Sub-id prefix renamed `seed-timeline-` → `follow-timeline-`. On sign-in, `retarget_timeline` emits a self-contacts REQ so the timeline can open even when sign-in happens after startup.

## Consequences

- Timeline is empty until the user signs in — no pre-seeded content from hardcoded accounts
- Sign-in path now responsible for priming the timeline via retarget_timeline → self-contacts REQ
- Legacy `seed-timeline-` prefix kept alive in the EOSE handler for in-flight subscriptions
- Diagnostic surface label updated from SeedTimeline to FollowTimeline
- Test fixtures that referenced FIATJAF_PUBKEY/JB55_PUBKEY converted to local constants

## Open Tail

- User also directed: 'there should be no relay hardcoded for discovery, the app is supposed to provide an indexer relay' — `BOOTSTRAP_DISCOVERY_RELAYS` (including relay.damus.io) still exists and needs to be replaced with app-provided indexer relay URLs via FFI. A 6-step refactor plan was outlined but not yet executed.

## Evidence

- transcript lines 45-45
- transcript lines 316-332
- transcript lines 334-334
- transcript lines 788-825
- transcript lines 1017-1025
- transcript lines 1094-1100

