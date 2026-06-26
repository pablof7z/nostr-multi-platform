---
type: episode-card
date: 2026-06-26
session: 55264cfe-6420-4b06-a655-e0a935729211
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/55264cfe-6420-4b06-a655-e0a935729211.jsonl
salience: architecture
status: active
subjects:
  - release-manifest
  - public-crate-registry
  - crate-classification
supersedes: []
related_claims: []
source_lines:
  - 3083-3090
  - 3105-3160
  - 3189-3201
  - 3198-3200
captured_at: 2026-06-26T11:58:39Z
---

# Episode: Release manifest as explicit system-of-record for public crates — no auto-discovery

## Prior State

The new `nmp-nip89` crate was created but not registered in `release/nmp-release.toml`, leaving it undeclassified in the release manifest.

## Trigger

CI failure 'release manifest + package dry-run' (line 3084) flagged that the workspace package was unclassified in the manifest.

## Decision

Add `nmp-nip89` as a `[[public_crates]]` entry in the release manifest (line 3141–3143), following the peer NIP crate format (nip42-types, nip92-types, etc.) and positioned alphabetically among public crates.

## Consequences

- Release manifest is the authoritative system-of-record; every workspace crate must be explicitly classified as public or private
- No auto-discovery. New crates block CI until manually registered; the gate catches accidental crate creation
- Establishes durable gate: PRs introducing new public crates cannot land without manifest registration
- The manifest entry determines which crates are shipped to registry / binaries
- Future crate additions require manifest update as a blocking step

## Open Tail

*(none)*

## Evidence

- transcript lines 3083-3090
- transcript lines 3105-3160
- transcript lines 3189-3201
- transcript lines 3198-3200

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-26-5-release-manifest-as-explicit-system-of.json`](transcripts/2026-06-26-5-release-manifest-as-explicit-system-of.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-26-5-release-manifest-as-explicit-system-of.json`](transcripts/raw/2026-06-26-5-release-manifest-as-explicit-system-of.json)
