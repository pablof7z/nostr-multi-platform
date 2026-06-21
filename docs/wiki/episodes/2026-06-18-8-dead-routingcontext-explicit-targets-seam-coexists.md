---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: active
subjects:
  - explicit-targets
  - publish-path
  - routing-seam
supersedes: []
related_claims: []
source_lines:
  - 230-240
  - 604-614
captured_at: 2026-06-18T19:42:43Z
---

# Episode: Dead RoutingContext::explicit_targets seam coexists with live PublishTarget::Explicit

## Prior State

Two parallel publish-side explicit-relay mechanisms exist: the live `PublishTarget::Explicit` (PublishPlan → ActorCommand → publish_signed_to → PublishTarget::Explicit, correctly preserving host relay through sign/park/validate/dispatch) and the dead `RoutingContext::explicit_targets` (always None in mailboxes.rs; the router seam exists but is never populated).

## Trigger

#1493 audit finding P7 #3/#4; codex-design-first confirmed NIP-29 PublishPlan is correct via the live seam, and that editing only the NIP-29 side would create dead plumbing.

## Decision

Filed follow-up issue #1538 for architectural unification. Two options documented: (A) delete the dead RoutingContext::explicit_targets seam, or (B) migrate PublishTarget::Explicit to route through route_publish with explicit_targets populated. No immediate code change; NIP-29 PublishPlan left untouched.

## Consequences

- Debt tracked and labeled area:architecture, category:violation, doctrine:d0
- No half-compiling dead plumbing introduced
- NIP-29 PublishPlan confirmed correct via live seam

## Open Tail

- Architectural call needed: delete dead seam or unify through route_publish

## Evidence

- transcript lines 230-240
- transcript lines 604-614

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-8-dead-routingcontext-explicit-targets-seam-coexists.json`](transcripts/2026-06-18-8-dead-routingcontext-explicit-targets-seam-coexists.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-8-dead-routingcontext-explicit-targets-seam-coexists.json`](transcripts/raw/2026-06-18-8-dead-routingcontext-explicit-targets-seam-coexists.json)
