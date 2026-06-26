---
type: episode-card
date: 2026-06-26
session: 55264cfe-6420-4b06-a655-e0a935729211
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/55264cfe-6420-4b06-a655-e0a935729211.jsonl
salience: product
status: active
subjects:
  - user-agent
  - nip-89-client-tag
  - client-identity
  - composition-root
supersedes: []
related_claims: []
source_lines:
  - 1-93
  - 2471-2483
  - 3423-3430
captured_at: 2026-06-26T11:58:39Z
---

# Episode: Single ClientIdentity source-of-truth for app identity across User-Agent and NIP-89

## Prior State

NMP hardcodes User-Agent to `nmp/<version>` with no mechanism for apps to declare identity; NIP-89 client tag not settable by apps.

## Trigger

User asks whether NMP allows apps to set User-Agent (line 5) so relays identify the app; also notes NIP-89 client tag should be settable (line 43).

## Decision

Design a single `ClientIdentity { name, version, handler }` declaration at the composition root as the authoritative source for both User-Agent (transport layer) and NIP-89 client tag (protocol layer), with two separate consumers that respect the distinction.

## Consequences

- User-Agent becomes configurable at app level, rendering as 'Name/version (nmp/version)', with fallback to `nmp/<version>` when unset
- NIP-89 client tag becomes opt-in, injected at a single publish decision site only
- Single source-of-truth principle enforced (no duplicate 'Chirp' declarations)
- Composition root owns app identity; nmp-network receives UA as inert config, respecting layering

## Open Tail

- Chirp ships the opt-in tag; other apps may vary

## Evidence

- transcript lines 1-93
- transcript lines 2471-2483
- transcript lines 3423-3430

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-26-1-single-clientidentity-source-of-truth-for.json`](transcripts/2026-06-26-1-single-clientidentity-source-of-truth-for.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-26-1-single-clientidentity-source-of-truth-for.json`](transcripts/raw/2026-06-26-1-single-clientidentity-source-of-truth-for.json)
