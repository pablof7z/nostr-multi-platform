---
type: episode-card
date: 2026-06-12
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: product
status: active
subjects:
  - nmp-ffi
  - c-abi
  - author-view
  - thread-view
  - doctrine-d0
  - open-interest
supersedes: []
related_claims: []
source_lines:
  - 3622-3628
  - 3645-3667
  - 3676-3688
  - 3879-3906
captured_at: 2026-06-12T06:14:07Z
---

# Episode: Legacy {1,6} C-ABI surfaces + author/thread state machine retired

## Prior State

Kernel-resident AuthorViewState/ThreadViewState state machine existed alongside hardcoded Chirp social-kind default {1,6} inside the generic FFI layer (a D0 violation). Four nmp_app_open/close_author/thread C-ABI symbols exposed these to platform shells. Projection registry had 36 keys (30 with Swift decoders).

## Trigger

#958/#957; ADR-0042 authorized the deletion; the {1,6} hardcoded kinds were a doctrine-0 violation that should never have been in the generic FFI layer

## Decision

Delete all four C-ABI symbols (nmp_app_open_author, close_author, open_thread, close_thread), the AuthorViewState/ThreadViewState state machine, author_view (KAVW) and thread_view (KTVW) typed projections, their FlatBuffers schemas, and the generated Swift readers. Migration path: open_interest with verbatim NIP-01 filters. Claimed_profiles decode cluster promoted to public typed surface.

## Consequences

- C-ABI break forced version bump from 0.3.1 to 0.4.0 with a BREAKING changelog entry
- Projection registry drops from 36 to 34 total keys (28 with Swift decoders)
- −5,750 LOC across 66 files
- The merge with just-merged #1096 silently broke open_view_pins() (7 compilation errors, zero conflict markers) — caught only by Opus review
- 4 ram_eviction tests migrated from open_thread/open_author to open_interest seam rather than deleted
- Example-compile gap class identified: cargo build --workspace does not build examples, allowing visibility bugs to slip CI — now in validation playbook

## Open Tail

*(none)*

## Evidence

- transcript lines 3622-3628
- transcript lines 3645-3667
- transcript lines 3676-3688
- transcript lines 3879-3906

