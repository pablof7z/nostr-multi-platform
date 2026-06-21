---
type: episode-card
date: 2026-05-26
session: 6e4c3a3a-9515-4437-a4bf-b4228a10ae57
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/6e4c3a3a-9515-4437-a4bf-b4228a10ae57.jsonl
salience: architecture
status: active
subjects:
  - open-timeline-command
  - follow-feed-kinds
  - logical-interest-source
supersedes: []
related_claims: []
source_lines:
  - 791-793
  - 837-844
  - 845-848
  - 1087-1129
captured_at: 2026-06-18T05:41:38Z
---

# Episode: Contact-list subscription kinds moved from kernel-hardcoded to host-declared

## Prior State

The kernel (nmp-core) hardcoded `kinds: {1, 6}` (short notes + reposts) inside `follow_feed_interest()`. `ActorCommand::OpenTimeline` was a unit variant with zero arguments — the substrate unilaterally decided what event kinds to fetch. This was a D0 violation: Chirp-specific social knowledge baked into the app-agnostic substrate.

## Trigger

User identified that hardcoding kinds {1,6} ties social-app semantics into the core substrate: 'I think that's the wrong level of abstraction — that means we are tying kinds:1,6 as something special in the core.' This matched an existing backlog entry (V-45) but with no implementation path.

## Decision

Replace `ActorCommand::OpenTimeline` with `OpenContactListSubscription { kinds: BTreeSet<u32> }` so the host app declares which kinds it wants. The kernel stores `follow_feed_kinds` (starts empty; empty = withdraw all interests). InterestId hashing now includes the kinds set so two apps with different kind sets over the same viewer don't collide. The C ABI symbol `nmp_app_open_timeline` is unchanged — it just declares `{1, 6}` internally.

## Consequences

- The substrate no longer assumes every app wants short notes and reposts; a podcast app can pass `{31337}` or any other kind set.
- 19 existing kernel/actor tests had to be updated to explicitly declare `follow_feed_kinds = {1, 6}` — surfacing the hidden contract production already honored.
- InterestId hash change means a one-time CLOSE+REQ cycle on upgrade (accepted cost).
- `follow_feed_kinds` resets to empty on `ActorCommand::Reset` — the host must re-declare on timeline re-entry, which is D0-correct.
- Auto-include-viewer behavior was identified as app policy (not kernel contract); left as-is for now but flagged for future removal.
- V-45 backlog item reworded from 'SocialTimeline' to 'NIP-02 contact-list author expansion' to avoid social-app vocabulary in the substrate.
- PR #728 merged; all CI checks passing including doctrine grep gates, C-ABI freeze, and FFI header drift.

## Open Tail

- Future: a typed FFI surface could let hosts pass arbitrary kinds through the C ABI (today `nmp_app_open_timeline` is the Chirp-specific declaration site hardcoding {1,6}).
- Optional naming follow-up: `follow_feed_interest_ids` kernel field could rename to `contact_list_authors_interest_ids` for consistency.

## Evidence

- transcript lines 791-793
- transcript lines 837-844
- transcript lines 845-848
- transcript lines 1087-1129

