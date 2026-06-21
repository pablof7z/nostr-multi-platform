---
type: episode-card
date: 2026-05-26
session: 37e351ee-aa2b-43eb-9793-482de338f883
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/37e351ee-aa2b-43eb-9793-482de338f883.jsonl
salience: product
status: active
subjects:
  - swift-snake-case-decoder
  - flatbuffers-decode
supersedes:
  - 2026-05-26-3-flatbuffers-decoder-snake-case-conversion-preserves
related_claims: []
source_lines:
  - 440-473
captured_at: 2026-06-18T05:53:12Z
---

# Episode: Swift snake-case conversion preserves leading/trailing underscores

## Prior State

convertFromSnakeCase converted the entire key including leading and trailing underscores, which could cause private-looking fields (e.g. __rev, last_name_) to alias public field names after conversion.

## Trigger

Review feedback identified the aliasing risk between private and public field names when underscores at key boundaries were consumed by the conversion.

## Decision

Leading and trailing underscores are now stripped before snake-case conversion, the body between them is converted, and the underscores are reattached. This matches the subset of Rust's JSONDecoder.convertFromSnakeCase used by snapshot keys.

## Consequences

- Keys like __rev and last_name_ will preserve their underscore markers, preventing private/public field name collisions
- Wire format keys with boundary underscores will decode to different Swift property names than before — a breaking change for any consumer relying on the old conversion

## Open Tail

*(none)*

## Evidence

- transcript lines 440-473

