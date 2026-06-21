---
type: episode-card
date: 2026-05-26
session: 54fc9b94-b995-46c6-8372-59c4abe0f95a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/54fc9b94-b995-46c6-8372-59c4abe0f95a.jsonl
salience: product
status: superseded
subjects:
  - swift-flatbuffers-key-decoding
  - snake-case-semantics
supersedes: []
related_claims: []
source_lines:
  - 639-677
captured_at: 2026-06-18T05:51:50Z
---

# Episode: FlatBuffers decoder snake-case conversion preserves leading/trailing underscores

## Prior State

Swift FlatBuffers keyed decoding's convertFromSnakeCase stripped all underscores uniformly, diverging from Rust's serde convertFromSnakeCase and allowing future private-looking fields (e.g., __field) to alias public field names

## Trigger

Review finding that key aliasing could occur when Rust-side snapshot keys use leading/trailing underscores for private fields

## Decision

Preserve leading and trailing underscores through the snake-case conversion; only transform underscores between words. This matches Rust's serde convertFromSnakeCase semantics

## Consequences

- Keys like __privateField__ no longer alias with privateField
- Swift decoder key semantics now match Rust's serde behavior
- Future private-looking field names are safe from aliasing collisions

## Open Tail

*(none)*

## Evidence

- transcript lines 639-677

