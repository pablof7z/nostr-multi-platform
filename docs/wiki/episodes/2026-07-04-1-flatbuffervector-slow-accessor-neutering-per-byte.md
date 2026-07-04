---
type: episode-card
date: 2026-07-04
session: f308bb0b-7b74-4684-9a5b-1fce8ffcab35
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/f308bb0b-7b74-4684-9a5b-1fce8ffcab35.jsonl
salience: root-cause
status: active
subjects:
  - flatbuffervector-slow-accessor
  - bulk-memcpy-mandate
  - codegen-drift-gate
supersedes: []
related_claims: []
source_lines:
  - 37-54
  - 1039-1041
  - 1042-1046
  - 1204-1220
  - 1313-1321
  - 1341-1390
  - 1464-1481
  - 1503-1505
  - 1848-1851
  - 1944-1950
captured_at: 2026-07-04T11:47:00Z
---

# Episode: FlatbufferVector slow-accessor neutering: per-byte copy banned, bulk-pointer copy mandated

## Prior State

Generated Swift FlatBuffers byte-vector accessors (FlatbufferVector<UInt8>) had a default var accessor that, when passed to Data(_:) or Data(vector.map { $0 }), iterated the vector per-byte — effectively an O(n) element-by-element copy on every decode. Multiple call sites across generated and hand-written Swift used this pattern silently. The app would peg at ~195% CPU in a sustained busy loop under real data.

## Trigger

User reported the running Chirp iOS app was 'getting increasingly slow' / 'completely unresponsive.' CPU sampling (sample tool) of PID 89962 showed 195% sustained CPU, with the hot path in nostr_database copy_nonoverlapping precondition checks and ByteBuffer readWithUnsafeRawPointer — confirming the per-byte FlatbufferVector<UInt8> iteration as the root cause.

## Decision

All 8 FlatbufferVector<UInt8> accessors with a fast withUnsafePointerTo* sibling were annotated @available(*, unavailable, ...) via an idempotent neuter script, turning latent per-byte misuse into hard compile errors. Every call site was migrated to the bulk withUnsafePointerTo* + memcpy pattern. The Rust codegen template (swift_keyed_cache.rs) that generated one of the buggy call sites was also fixed so regeneration cannot reintroduce the pattern. A zero-copy 'keep references into parent buffer' redesign was explicitly evaluated and rejected as architecturally worse (unbounded retained frame memory, aliasing hazards, no improvement to the real UniFFI memcpy bottleneck). A CI gate (check-flatbuffer-byte-vector-accessors.sh) was added to codegen-drift.yml to prevent future regression.

## Consequences

- The @available annotation immediately surfaced two additional silent instances of the exact same bug that compile-checks could not previously catch: KeyedRefCache.generated.swift (2 call sites, generated from nmp-codegen Rust template) and TypedProjectionGlueEmbed.swift (2 call sites, hand-written).
- The Rust codegen template (swift_keyed_cache.rs) was fixed at source, so cargo run -p nmp-codegen -- gen keyed-ref-cache produces the corrected output — drift-check stays green on next regen.
- After the fix, the near-empty simulator settled to <1% idle CPU; scroll-driven spikes were 5-11% (normal SwiftUI render cost) returning to 0% immediately.
- On the data-heavy simulator ('iPhone 16 ci'), a different ~3.5-minute post-launch spike (150-220% CPU) was observed — profiled as crypto ops, LMDB queries, and hex/regex parsing across many threads (post-reinstall resync), not the same single-path busy loop. It self-resolved to mostly idle.
- The neuter script was wired into regen-flatbuffers.sh so it runs automatically after every flatc regeneration.
- 155 nmp-codegen tests and 210 doctrine-lint smoke tests pass.

## Open Tail

- The ~3.5-minute post-launch resync spike on data-heavy accounts was diagnosed as different in nature but not fully investigated or fixed — the user has not confirmed whether it is acceptable.
- Pre-existing ADR-number drift (ADR-0070 vs ADR-0063) in swift_keyed_cache.rs was noted but not addressed.
- The user's original 'still pegged' complaint was traced to the second simulator running the old binary; after rebuild the app showed a bounded resync spike but the user has not confirmed full resolution.

## Evidence

- transcript lines 37-54
- transcript lines 1039-1041
- transcript lines 1042-1046
- transcript lines 1204-1220
- transcript lines 1313-1321
- transcript lines 1341-1390
- transcript lines 1464-1481
- transcript lines 1503-1505
- transcript lines 1848-1851
- transcript lines 1944-1950

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-07-04-1-flatbuffervector-slow-accessor-neutering-per-byte.json`](transcripts/2026-07-04-1-flatbuffervector-slow-accessor-neutering-per-byte.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-07-04-1-flatbuffervector-slow-accessor-neutering-per-byte.json`](transcripts/raw/2026-07-04-1-flatbuffervector-slow-accessor-neutering-per-byte.json)
