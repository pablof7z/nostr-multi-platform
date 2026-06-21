---
type: episode-card
date: 2026-06-19
session: e6b44a84-8cfc-48b2-863a-58382398b5df
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/e6b44a84-8cfc-48b2-863a-58382398b5df.jsonl
salience: root-cause
status: superseded
subjects:
  - nip02-follow-list
  - nip65-resolver
  - kernel-event-observer
supersedes: []
related_claims: []
source_lines:
  - 378-381
  - 549-549
  - 773-773
  - 783-795
captured_at: 2026-06-19T12:27:38Z
---

# Episode: Follow-list stale UI and broken action — dual root-cause diagnosis

## Prior State

Follow button was assumed functional: profiles of followed users should display 'Following', and tapping 'Follow' should publish a kind:3 that updates relay state and local UI via the FollowListProjection.

## Trigger

User testing on physical device revealed (1) already-followed profiles show 'Follow' and (2) tapping 'Follow' produces haptic but button stays 'Follow' with no visible state change.

## Decision

Diagnosed two distinct root causes: (a) KernelEventObserver is a live-only feed — it does NOT replay existing LMDB kind:3 data when FollowListProjection is registered, so the follow set is empty until the next relay push; (b) nip65_resolver step 4 applies recipient-inbox fan-out unconditionally for any p-tagged event, but kind:3 p-tags are follows (not DM recipients), so the published kind:3 routes to the wrong relay set and may never propagate. Proposed fix for (b) is a `!is_discovery_kind(kind)` guard on the recipient fan-out branch.

## Consequences

- Kind:3 publish routing must be carved out of the generic p-tag recipient fan-out — discovery kinds use the indexer lane instead
- FollowListProjection needs initial-state hydration from the existing LMDB contacts cache at registration time, not just live-event feed
- Read-your-writes invariant for locally-published kind:3 must be verified: publish → EventIngestDispatcher → observer pipeline must flow back to the projection synchronously

## Open Tail

- Fix not yet implemented — awaiting alignment on approach before touching code
- Need to confirm whether locally-published kind:3 events actually traverse the EventIngestDispatcher → KernelEventObserver pipeline (read-your-writes path)
- FollowListProjection initial-hydration mechanism not yet designed

## Evidence

- transcript lines 378-381
- transcript lines 549-549
- transcript lines 773-773
- transcript lines 783-795

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-1-follow-list-stale-ui-and-broken.json`](transcripts/2026-06-19-1-follow-list-stale-ui-and-broken.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-1-follow-list-stale-ui-and-broken.json`](transcripts/raw/2026-06-19-1-follow-list-stale-ui-and-broken.json)
