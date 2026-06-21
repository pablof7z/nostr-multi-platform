---
type: episode-card
date: 2026-05-26
session: e4861768-9a00-4d83-b7a3-a39d07749d1c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/e4861768-9a00-4d83-b7a3-a39d07749d1c.jsonl
salience: architecture
status: active
subjects:
  - flatbuffers-transport
  - nmp-update-envelope
  - nmp-ffi-update-callback
supersedes: []
related_claims: []
source_lines:
  - 1-27
  - 102-124
  - 165-203
  - 1683-1803
  - 2295-2321
  - 2680-2745
captured_at: 2026-06-18T05:55:20Z
---

# Episode: FlatBuffers replaces JSON as kernel-to-host update transport

## Prior State

Kernel-to-host update transport used JSON strings delivered through C-ABI callbacks; all platforms (Rust FFI, WASM/web, iOS Swift, Android Kotlin) consumed JSON snapshots

## Trigger

PR 582 proposes replacing JSON with canonical FlatBuffers binary frames; session validates through cross-platform build/test: Rust unit tests (8/8), chirp-tui e2e against live relay (42/42 unit + rexpect PTY), TypeScript vitest (20/20), Android JVM golden-hex parity, Swift schema conformance

## Decision

FlatBuffers binary frames with `NMPU` file identifier adopted as the canonical kernel→host update transport; `Kernel::make_update` emits `Vec<u8>` FlatBuffers; `Snapshot` and `Panic` frame kinds replace JSON; FFI callback signature changed from string to `(*const u8, usize)` byte-pointer pair; schema version pin enforced asymmetrically per platform runtime (Rust+Swift 25.12.19, TypeScript 25.9.23, Kotlin 25.2.10)

## Consequences

- Schema-version drift gate enforced by ci/check-flatbuffers-version-pins.sh
- Web delivery changed from JSON number array to Uint8Array binary
- Decoder hardening added: malformed frames rejected as invalid_flatbuffer, schema version mismatch degrades gracefully
- Browser wasm relay path has pre-existing crashes (separate issue)
- Native chirp-tui path empirically validated end-to-end with real relay connection
- PR merged at d73e048b with 6-commit history preserved

## Open Tail

- Browser wasm panic (Instant::now + RefCell double-borrow) filed as separate issue
- iOS xcodebuild validation deferred to CI

## Evidence

- transcript lines 1-27
- transcript lines 102-124
- transcript lines 165-203
- transcript lines 1683-1803
- transcript lines 2295-2321
- transcript lines 2680-2745

