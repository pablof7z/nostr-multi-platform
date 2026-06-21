---
type: episode-card
date: 2026-06-19
session: e6b44a84-8cfc-48b2-863a-58382398b5df
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/e6b44a84-8cfc-48b2-863a-58382398b5df.jsonl
salience: root-cause
status: active
subjects:
  - nip65-resolver
  - kind3-routing
  - discovery-kind
supersedes: []
related_claims: []
source_lines:
  - 773-787
  - 1004-1023
captured_at: 2026-06-19T12:38:13Z
---

# Episode: kind:3 discovery-kind events incorrectly routed to p-tagged recipient inboxes

## Prior State

Nip65OutboxResolver step 4 applies recipient-inbox fan-out unconditionally to ALL events with < 15 p-tags, treating every p-tag as a recipient whose inbox relays should receive the event.

## Trigger

User tested Chirp on device and observed kind:3 contact-list events being published to followees' inbox relays — p-tags in kind:3 are follows, not recipients.

## Decision

Discovery kinds (kind:0, kind:3, kind:10000–19999) must skip recipient inbox fan-out. The fix is a `!is_discovery_kind(kind)` guard on step 4 of `nip65_resolver.rs`, so p-tagged inbox fan-out only applies to non-discovery event kinds where p-tags truly represent recipients.

## Consequences

- All discovery-kind publishes (profile metadata, contact lists, replaceable lists) will stop being sent to p-tagged users' read relays
- The `is_discovery_kind()` function already exists in `nmp-router/src/discovery.rs` and covers exactly the right set
- Users with < 15 follows were especially affected due to the RECIPIENT_INBOX_FANOUT_PTAG_THRESHOLD = 15 gate

## Open Tail

- Fix not yet implemented — user requested discussion before any code changes

## Evidence

- transcript lines 773-787
- transcript lines 1004-1023

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-1-kind-3-discovery-kind-events-incorrectly.json`](transcripts/2026-06-19-1-kind-3-discovery-kind-events-incorrectly.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-1-kind-3-discovery-kind-events-incorrectly.json`](transcripts/raw/2026-06-19-1-kind-3-discovery-kind-events-incorrectly.json)
