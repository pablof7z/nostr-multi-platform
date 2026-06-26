---
type: episode-card
date: 2026-06-26
session: 55264cfe-6420-4b06-a655-e0a935729211
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/55264cfe-6420-4b06-a655-e0a935729211.jsonl
salience: architecture
status: active
subjects:
  - nip-89-privacy
  - publish-decision-site
  - kind-classifier
  - metadata-isolation
supersedes: []
related_claims: []
source_lines:
  - 71-72
  - 2781-2807
  - 2992-3016
  - 3041-3044
captured_at: 2026-06-26T11:58:39Z
---

# Episode: Privacy gate for NIP-89 metadata — single decision site, public events only, never encrypted

## Prior State

No mechanism existed to ensure NIP-89 tags only appear on public events; risk of client metadata leaking into encrypted DMs or gift-wraps.

## Trigger

Design requirement (line 71) that NIP-89 tags 'must never leak onto private events'; Opus reviewer identifies privacy as 'highest priority' (line 2781).

## Decision

Implement `finalize_outbound_tags` as the single, authoritative decision site that appends client tags only when `classify_publish_behavior(kind) == PublicRoutable`. All publish paths (auto and explicit arms) route through this chokepoint; hard exclusion of DMs (kind 14), gift-wraps (kind 1059), profile (kind 0), and reserved builder-only kinds.

## Consequences

- Privacy gate is deterministic and centralized; no bypass path exists for public-event publishes
- Pre-signed events (which cannot be mutated) correctly bypass the gate; their D10 routing gate still protects them
- Tag-before-sign ordering enforced (tags in signed payload, then hashed)
- Both publish arms (unsigned + unsigned-to-relays) proven by Group D integration tests to respect the gate
- 8 unit tests pin each kind's classify result; classifier becomes immutable without test break

## Open Tail

*(none)*

## Evidence

- transcript lines 71-72
- transcript lines 2781-2807
- transcript lines 2992-3016
- transcript lines 3041-3044

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-26-2-privacy-gate-for-nip-89-metadata.json`](transcripts/2026-06-26-2-privacy-gate-for-nip-89-metadata.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-26-2-privacy-gate-for-nip-89-metadata.json`](transcripts/raw/2026-06-26-2-privacy-gate-for-nip-89-metadata.json)
