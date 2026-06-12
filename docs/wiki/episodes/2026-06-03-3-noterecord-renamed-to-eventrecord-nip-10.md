---
type: episode-card
date: 2026-06-03
session: 7f143c67-6e46-424a-90a8-5bf844947fee
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/7f143c67-6e46-424a-90a8-5bf844947fee.jsonl
salience: architecture
status: active
subjects:
  - nip-10
  - type-naming
  - nmp-nip01
supersedes: []
related_claims: []
source_lines:
  - 936-942
captured_at: 2026-06-11T22:58:15Z
---

# Episode: NoteRecord renamed to EventRecord — NIP-10 is kind-agnostic

## Prior State

The struct for building NIP-10 reply tags was called `NoteRecord` in `nmp-nip01::decode`, implying it is specific to kind:1 notes.

## Trigger

User correction: NIP-10 threading applies to any event kind, not just kind:1. The type name `NoteRecord` is wrong — it should be `EventRecord`.

## Decision

Rename `NoteRecord` → `EventRecord` and audit the `nmp-nip01` crate name itself (it wraps kind-agnostic logic under a kind:1 name).

## Consequences

- The Rust migration (#918) proceeded using the current `NoteRecord` name; the rename is tracked as a follow-up
- The `Note::reply_to` builder and its input type now have an acknowledged naming mismatch that must be resolved

## Open Tail

- Rename `NoteRecord` → `EventRecord` in `nmp-nip01/src/decode.rs`
- Audit whether `nmp-nip01` crate itself should be renamed or split (kind-agnostic reply/tag logic vs kind:1-specific note logic)

## Evidence

- transcript lines 936-942

