---
type: episode-card
date: 2026-07-03
session: dcc80382-bcc0-45ea-8b9c-1a2fc741f872
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/dcc80382-bcc0-45ea-8b9c-1a2fc741f872.jsonl
salience: architecture
status: active
subjects:
  - read-lifecycle-engine
  - read-host-seam
  - concept-doorways
  - nmp-read-session
  - declarativeness-test
supersedes:
  - 2026-07-03-1-read-model-collapse-concept-owned-active
related_claims: []
source_lines:
  - 69-109
  - 1031-1052
  - 1115-1123
  - 1183-1191
  - 1295-1305
  - 1557-1558
captured_at: 2026-07-03T09:43:37Z
---

# Episode: Read-lifecycle engine spine: one internal engine with concept-crate doorways

## Prior State

Each concept had its own lifecycle machinery (registries, close maps, replay logic, teardown recipes). Doorways like open_search were defined in nmp-native-runtime as NmpApp methods, creating a pattern where adding a new concept meant adding a method to the runtime crate and hard-dependency on the concept's NIP crate.

## Trigger

The collapse doctrine requiring one internal lifecycle engine; during implementation, the dependency direction constraint was discovered — concept → engine ← runtime — meaning open_<concept> doorways cannot live in the runtime crate without creating circular or inappropriate dependencies.

## Decision

New Layer-4 crate nmp-read-session owns the single lifecycle implementation: ReadSessionRegistry (handle allocation, open/close, reverse teardown, one leak audit), open_read/close_read (replay-before-live, exact per-demand withdrawal, tombstoning), and the ReadHost seam. NmpApp implements ReadHost once, generically — no per-concept method. Concept doorways (open_replies, open_reactions, open_reposts, open_zaps) live in their own concept crates and consume the seam. Declarativeness test: concept crates contain no registry, close map, replay, or teardown code — open bodies run 43-50 non-comment lines each. nmp-replies created as a new concept crate (not in nip01/nip22) because 'replies' spans NIP-10 kind:1 AND NIP-22 kind:1111, setting the precedent for nmp-reactions, nmp-reposts, nmp-zaps. Lint ratchet planned: no open_<concept> defined outside its concept crate, no nmp-nip* deps in runtime crates.

## Consequences

- PR #2801 merged: engine spine + open_replies on master, feed rides the spine with zero behavior change (116 feed tests as regression harness)
- Feed and replies share one leak audit in one registry
- All four concept reads shipped as separate concept crates: nmp-replies (#2801), nmp-reposts (#2816), nmp-reactions (#2817), nmp-zaps (#2820) — all merged
- nmp-zaps classified as private package (not public release train) per #2318 settling zaps as post-v1
- Doorway relocation executed by parallel fleet for existing lanes: search moved to nmp-nip50, group reads to nmp-nip29, group reactions folded into nmp-reactions, native-runtime per-NIP deps feature-gated (#2797, 10+ merged PRs)
- #2777's 'no exempt lanes' verified real: SearchSessionRegistry has zero references, nmp-nip50 depends on nmp-read-session, group-feed doorway files gone
- #2797 still open: nmp-nip05 and nmp-nip18 were last unconditional deps, being fixed by #2852

## Open Tail

- #2797 nearly complete but not yet closed (nmp-nip05/nmp-nip18 being made optional by another fleet)
- Reads are callable from Rust but have no FFI/wasm export — consumers must wire them through their own app-owned facades per #2763

## Evidence

- transcript lines 69-109
- transcript lines 1031-1052
- transcript lines 1115-1123
- transcript lines 1183-1191
- transcript lines 1295-1305
- transcript lines 1557-1558

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-07-03-2-read-lifecycle-engine-spine-one-internal.json`](transcripts/2026-07-03-2-read-lifecycle-engine-spine-one-internal.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-07-03-2-read-lifecycle-engine-spine-one-internal.json`](transcripts/raw/2026-07-03-2-read-lifecycle-engine-spine-one-internal.json)
