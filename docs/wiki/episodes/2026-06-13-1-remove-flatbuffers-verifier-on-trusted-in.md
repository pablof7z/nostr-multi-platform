---
type: episode-card
date: 2026-06-13
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: active
subjects:
  - flatbuffers-decode-path
  - ios-projection-decoder
  - codegen-template
supersedes:
  - 2026-06-13-2-flatbuffers-verifier-removed-on-trusted-in
related_claims: []
source_lines:
  - 5287-5289
  - 5356-5360
  - 5527-5545
  - 5566-5603
captured_at: 2026-06-13T21:09:24Z
---

# Episode: Remove FlatBuffers Verifier on trusted in-process decode path

## Prior State

All 29 generated typed decoders and the top-level KernelUpdateFrameDecoder used getCheckedRoot (the verified FlatBuffers path), running an O(N) recursive Verifier walk on every 4 Hz snapshot frame — on data produced by the same Rust process microseconds earlier across a trusted in-process FFI boundary. This was the codegen default, never deliberately chosen.

## Trigger

Performance investigation (Time Profiler on debug build, then code audit) revealed the Verifier as pure waste on trusted data — no rationale existed for choosing getCheckedRoot over the unchecked getRoot.

## Decision

Switch all trusted-path decoders from getCheckedRoot to unchecked getRoot. Fix at the codegen template (crates/nmp-codegen/src/swift_typed_decoders.rs) so it sticks; regenerate all 28 generated sites plus 6 hand-written decoders. The 4-byte file-identifier magic check is dropped (key+schemaId envelope routing already selects the correct buffer). Android was already unchecked (Java getRootAs*).

## Consequences

- Per-frame Verifier walk eliminated on iOS — the single most prominent named cost in the time profile after the debug-build multiplier
- Decoding becomes infallible (no try?/guard let); presence is determined solely by !bytes.isEmpty
- Garbled-file-identifier test cases removed (they tested verifier rejection, which no longer applies on this path)
- Opus review confirmed safe: content-tree buffers from relay content are re-encoded by the kernel before reaching getRoot, so no untrusted relay bytes ever hit unchecked decode
- One decoder (TypedGroupDefaultsDecoder) was missed by the agent's initial regeneration, causing a codegen-drift CI failure that had to be fix-forwarded (#1334)

## Open Tail

- Verify production performance on release builds with real relay traffic under the new decode path

## Evidence

- transcript lines 5287-5289
- transcript lines 5356-5360
- transcript lines 5527-5545
- transcript lines 5566-5603

