---
type: episode-card
date: 2026-05-18
session: d27a4f61-511b-4086-845d-335493f9b464
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/d27a4f61-511b-4086-845d-335493f9b464.jsonl
salience: reversal
status: active
subjects:
  - marmot-mls
  - nmp-marmot
  - roadmap-scope
supersedes: []
related_claims: []
source_lines:
  - 1-2
  - 248-252
captured_at: 2026-06-18T04:15:45Z
---

# Episode: Marmot/MLS milestone pulled forward from post-v1 deferral

## Prior State

marmot-mls.md explicitly scoped as post-v1 deferred work; M11.5 deliberately excludes encrypted groups; north-star memory states "complete v1 with zero debt"

## Trigger

User directive to "implement nmp-mls with marmot"; confirmed pulling it forward now, with full Chirp scope (Rust + iOS SwiftUI)

## Decision

Pull the entire Marmot/MLS encrypted-groups milestone into current work — nmp-marmot crate wrapping MDK plus full Chirp app integration (Rust + iOS SwiftUI)

## Consequences

- nmp-marmot crate created wrapping mdk-core / mdk-sqlite-storage from crates.io
- Roadmap deviation from v1-only principle; session log must record the deviation
- Full iOS SwiftUI consumer surface required (not just Rust library)

## Open Tail

- Group creation → publish key package → invite → accept → message exchange dogfood test still pending
- Keychain restore re-enabled only after login confirmed working

## Evidence

- transcript lines 1-2
- transcript lines 248-252

