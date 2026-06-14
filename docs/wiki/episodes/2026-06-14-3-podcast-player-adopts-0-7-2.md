---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: direction
status: active
subjects:
  - podcast-player
  - nmp-dependency-model
  - vendored-deps
  - git-dep
supersedes: []
related_claims: []
source_lines:
  - 7797-7831
captured_at: 2026-06-14T20:01:17Z
---

# Episode: Podcast-player adopts 0.7.2 git-dep model over vendored-blossom strategy

## Prior State

Another session had been migrating podcast-player to a vendored-blossom model at NMP 0.6.2: blossom copied into vendor/nmp-blossom as a workspace member with a path [patch], /tmp patch redirects, NMP pinned at fbc0155031. PRs #488, #498, #500 in the podcast repo were in flight.

## Trigger

Blossom un-parking (v0.7.2) made vendoring architecturally unnecessary. User explicitly chose option A: 'adopt the 0.7.2 git-dep model, drop the vendoring' (line 7807).

## Decision

Migrate podcast-player to the 0.7.2 git-dep model: re-pin all 6 NMP crates to 45ac8c3e4/0.7.2, delete the entire vendor/nmp-blossom directory and its [patch]/workspace-member entries, re-pin nmp-feedback to its merged 0.7.2 commit, verify single nmp-core version and green host + iOS-sim builds.

## Consequences

- Single nmp-core version (0.7.2) across the entire dependency graph — no duplicate-version builds
- No /tmp path-patch fragility; CI runners and other developers can build without reproducing local patch directories
- Vendoring PRs #488/#498/#500 in podcast-player repo are superseded by this approach
- The other session's in-flight vendoring work conflicts with this direction — coordination needed

## Open Tail

- podcast-player PR #501 was already CLOSED and conflicting; fresh PR needed on current main
- The other session that pursued vendoring likely doesn't know blossom got un-parked — needs notification

## Evidence

- transcript lines 7797-7831

