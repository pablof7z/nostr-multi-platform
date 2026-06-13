---
type: episode-card
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: architecture
status: superseded
subjects:
  - embed-host
  - nmp-ffi
  - d0-thin-shell
  - nmp-content
  - embed-projection
supersedes: []
related_claims: []
source_lines:
  - 8076-8082
  - 8126-8146
captured_at: 2026-06-13T19:35:42Z
---

# Episode: D0 thin-shell violation resolved — embed projection moves to nmp-ffi (#1283)

## Prior State

iOS EmbedHost.swift and the Android equivalent reimplemented the Rust embed-resolver in the shell (parsing nostr kinds, kind:0 profile JSON, NIP-23 tags, media URLs in Swift/Kotlin), violating D0 (shells hold zero protocol logic).

## Trigger

Agent analysis of D0 doctrine + zero-tolerance-on-hacks determined resolution must happen above the kernel (nmp-ffi can reach nmp-content; kernel cannot due to crate layering). F-CR-12 already half-built the consumer side.

## Decision

GO: resolve embeds in nmp-ffi via nmp-content, ship as typed projection on sidecar FlatBuffer key, shells decode without protocol logic. ~3-PR migration (M+S+S+verify).

## Consequences

- Shells will no longer contain protocol parsing logic — pure decode of typed projections
- Stops shell-duplication spreading (Android hadn't duplicated yet, preventing it now)
- Same pattern applies to #920 TimelineItem: resolve above kernel, ship typed
- F-CR-12 consumer-side work already partially built, reducing scope

## Open Tail

- #1291 (Chirp parity) blocked on #1283 and #980 landing first

## Evidence

- transcript lines 8076-8082
- transcript lines 8126-8146

