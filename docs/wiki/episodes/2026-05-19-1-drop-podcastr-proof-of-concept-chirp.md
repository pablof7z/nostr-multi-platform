---
type: episode-card
date: 2026-05-19
session: 12b3f443-3c2d-4e47-976a-7f4ceab75343
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/12b3f443-3c2d-4e47-976a-7f4ceab75343.jsonl
salience: reversal
status: active
subjects:
  - chirp-app
  - m11-milestone
  - proof-of-concept-apps
supersedes: []
related_claims: []
source_lines:
  - 39-41
captured_at: 2026-06-18T04:36:30Z
---

# Episode: Drop Podcastr proof-of-concept; Chirp is sole focus app

## Prior State

The project roadmap had Podcastr (M11) as the primary kernel-boundary proof-of-concept app, with Highlighter (M11.5) as a secondary proof. The README, memory index, and north-star doc all referenced Podcastr as the M11 acceptance vehicle.

## Trigger

User explicitly directed: 'we're going to skip podcast app and focus only on chirp — I think you've read obsolete docs.'

## Decision

Podcastr is no longer a target. Chirp (already partially built in the codebase) becomes the sole proof-of-concept app demonstrating that the kernel boundary works end-to-end. M11 scope shifts from Podcastr to Chirp.

## Consequences

- The README and memory index still reference Podcastr as M11 — they are now stale and need updating
- Milestone acceptance criterion (byte-for-byte verbatim copy of original app UI files) now applies to Chirp's shell, not Podcastr's
- The `apps/podcast` directory and `NmpPodcast` crate are now historical/unmaintained
- Chirp's iOS shell (Bridge, Features, MarmotBridge) becomes the reference FFI consumer

## Open Tail

- Memory index and README still say 'rebuild ../podcast as M11 kernel-boundary proof' — needs rewrite
- M11 and M11.5 milestone docs need rescoping to Chirp-only
- Whether to delete or archive the Podcastr app crate

## Evidence

- transcript lines 39-41

