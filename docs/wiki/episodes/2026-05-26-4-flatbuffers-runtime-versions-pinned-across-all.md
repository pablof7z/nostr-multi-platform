---
type: episode-card
date: 2026-05-26
session: 37e351ee-aa2b-43eb-9793-482de338f883
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/37e351ee-aa2b-43eb-9793-482de338f883.jsonl
salience: architecture
status: active
subjects:
  - flatbuffers-version-pins
  - ci-transport-integrity
supersedes: []
related_claims: []
source_lines:
  - 86-127
  - 128-145
captured_at: 2026-06-18T05:53:12Z
---

# Episode: FlatBuffers runtime versions pinned across all platforms in CI

## Prior State

No CI check enforced FlatBuffers runtime version consistency across platforms. Regenerating bindings with a mismatched flatc would produce runtime guard mismatches (e.g., FLATBUFFERS_25_2_10()) that only surfaced in downstream platform builds.

## Trigger

Review feedback noted that the intentionally-skewed runtime versions across platforms (Rust+Swift 25.12.19, Web/TS 25.9.23, Android/Kotlin 25.2.10) needed enforcement to prevent a developer from accidentally regenerating one platform's bindings with a different flatc.

## Decision

Added check-flatbuffers-version-pins.sh that pins each platform's FlatBuffers version in its lockfile/manifest and validates that generated Kotlin bindings contain the matching runtime guard macro call. Documented the intentional version skew in the .fbs schema file.

## Consequences

- Regenerating bindings with a mismatched flatc will fail CI before the mismatch reaches platform builds
- The asymmetric versioning is now documented in-schema as an intentional state, not an accident
- Adding a new platform binding requires updating both the pin check and the schema comment

## Open Tail

*(none)*

## Evidence

- transcript lines 86-127
- transcript lines 128-145

