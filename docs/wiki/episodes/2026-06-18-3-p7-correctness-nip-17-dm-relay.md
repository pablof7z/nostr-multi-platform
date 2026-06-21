---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: root-cause
status: superseded
subjects:
  - nip17-dm-relay
  - relay-bypass-selection
  - relay-pruning
supersedes: []
related_claims: []
source_lines:
  - 19-48
  - 37-38
  - 50-52
  - 149-150
captured_at: 2026-06-18T18:34:07Z
---

# Episode: P7 correctness: NIP-17 DM relay silently prunable

## Prior State

Nip17DmRelay was omitted from relay_bypasses_selection, meaning the relay selection/pruning logic could silently remove a user's DM inbox relay, causing message loss with no indication.

## Trigger

P7 finding in the audit identified this as a likely bug; user designated it as critical with codex design-first requirement.

## Decision

Nip17DmRelay must be included in relay_bypasses_selection so DM inbox relays are never pruned.

## Consequences

- p7-nip17-routing agent owns planner interest/selection, nip29 publish_plan, and core/mailboxes exclusively
- Also responsible for pt2 (DM-relay pruning bug) which is the same root cause surface
- Fix must preserve the invariant that designated DM relays survive any pruning pass

## Open Tail

- Agent must use codex design-first to determine correct bypass mechanics before implementation
- Must verify no other relay-role enums are similarly missing from bypass selection

## Evidence

- transcript lines 19-48
- transcript lines 37-38
- transcript lines 50-52
- transcript lines 149-150

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-3-p7-correctness-nip-17-dm-relay.json`](transcripts/2026-06-18-3-p7-correctness-nip-17-dm-relay.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-3-p7-correctness-nip-17-dm-relay.json`](transcripts/raw/2026-06-18-3-p7-correctness-nip-17-dm-relay.json)
