---
type: episode-card
date: 2026-06-13
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: superseded
subjects:
  - flatbuffers-decode
  - ios-ffi
  - nmp-codegen
supersedes:
  - 2026-06-13-3-flatbuffers-verifier-dropped-on-trusted-in
related_claims: []
source_lines:
  - 5527-5603
  - 5619-5624
captured_at: 2026-06-13T20:33:24Z
---

# Episode: FlatBuffers Verifier removed on trusted in-process decode path

## Prior State

Every 4Hz kernel snapshot frame called getCheckedRoot() (the O(N) FlatBuffers Verifier recursive walk) on the top-level UpdateFrame and all ~29 typed projection sub-buffers — buffers that the same-process Rust kernel produced microseconds earlier across a trusted in-process FFI boundary. This was the codegen default with no rationale documented for the choice.

## Trigger

Time Profiler on a physical iPhone showed ~6.1% of debug CPU (and proportional release cost) spent on verification of trusted in-process data. Analysis confirmed no untrusted bytes ever reach getRoot: the kernel re-encodes even relay-sourced content-tree data via FlatBufferBuilder, and routing-by-schemaId is the real type guard (the 4-byte fileId magic was a redundant second discriminator).

## Decision

Switch all trusted-path decoders from getCheckedRoot to unchecked getRoot, fixed at the codegen template level (nmp-codegen swift_typed_decoders.rs) so it sticks. PR #1329 merged after Opus review confirmed safety: content-tree buffers are kernel-re-encoded (no relay verbatim), atomic Vec<u8> delivery precludes partial-buffer UB, and schemaId routing already guards type selection. Android confirmed already unchecked (getRootAs*).

## Consequences

- 35 decode sites (29 generated + 6 hand-written) now use unchecked getRoot
- File-identifier magic check dropped — schemaId envelope routing is the sole type guard
- Garbled-file-identifier test cases removed (contract no longer holds on trusted path); empty-payload guard retained
- Codegen drift gate green; template test asserts positive getRoot pattern and negative getCheckedRoot

## Open Tail

*(none)*

## Evidence

- transcript lines 5527-5603
- transcript lines 5619-5624

